//! Telegram 平台适配器：裸 `reqwest` 调 Bot API（无 SDK 依赖，与项目零依赖哲学一致）。
//!
//! `poll` 用 `getUpdates` 长轮询（timeout=25s），`send` 用 `sendMessage`。
//! 失败时打印并退避 3s 后返回空（自愈：下一轮重试），不向上抛错打断网关。

use std::time::Duration;

use tokio::sync::Mutex;

use async_trait::async_trait;

use crate::error::GanyuError;
use crate::gateway::{InboundMessage, PlatformAdapter};
use crate::GanyuResult;

pub struct TelegramAdapter {
    client: reqwest::Client,
    api: String,
    /// 长轮询游标（已处理的最后 update_id + 1）。
    offset: Mutex<i64>,
}

impl TelegramAdapter {
    pub fn new(token: &str) -> Self {
        // 构建仅在 TLS 后端初始化失败时出错。降级为默认客户端而非 panic——
        // release 配置为 panic=abort，一次 panic 会让整个常驻网关进程（含其他平台）退出。
        let client = reqwest::Client::builder()
            .user_agent("ganyu-gateway")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let api = format!("https://api.telegram.org/bot{token}");
        TelegramAdapter {
            client,
            api,
            offset: Mutex::new(0),
        }
    }
}

#[async_trait]
impl PlatformAdapter for TelegramAdapter {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn poll(&self) -> GanyuResult<Vec<InboundMessage>> {
        let offset = *self.offset.lock().await;
        let resp = self
            .client
            .get(format!("{}/getUpdates", self.api))
            .query(&[
                ("offset", offset.to_string()),
                ("timeout", "25".to_string()),
                ("allowed_updates", r#"["message"]"#.to_string()),
            ])
            .send()
            .await;
        let updates: serde_json::Value = match resp {
            Ok(r) => r
                .error_for_status()
                .map_err(|e| GanyuError::Http(e.to_string()))?
                .json()
                .await
                .map_err(|e| GanyuError::Http(e.to_string()))?,
            Err(e) => {
                eprintln!("[gateway:telegram] getUpdates 错误: {e}，3s 后重试");
                tokio::time::sleep(Duration::from_secs(3)).await;
                return Ok(Vec::new());
            }
        };
        let mut out = Vec::new();
        if updates["ok"].as_bool() != Some(true) {
            // 协议层错误：退避后下一轮重试（自愈）。
            eprintln!("[gateway:telegram] getUpdates 返回错误: {updates}，3s 后重试");
            tokio::time::sleep(Duration::from_secs(3)).await;
            return Ok(out);
        }
        let Some(arr) = updates["result"].as_array() else {
            return Ok(out);
        };
        for upd in arr {
            if let Some(n) = upd["update_id"].as_i64() {
                *self.offset.lock().await = n + 1;
            }
            let Some(text) = upd["message"]["text"].as_str() else {
                continue;
            };
            let Some(chat_id) = upd["message"]["chat"]["id"].as_i64() else {
                continue;
            };
            let from = upd["message"]["from"]["username"]
                .as_str()
                .unwrap_or("user");
            let text = text.trim().to_string();
            if text.is_empty() {
                continue;
            }
            out.push(InboundMessage {
                chat_id: chat_id.to_string(),
                user: from.to_string(),
                text,
            });
        }
        Ok(out)
    }

    async fn send(&self, chat: &str, text: &str) -> GanyuResult<()> {
        let chat_id: i64 = chat
            .parse()
            .map_err(|_| GanyuError::InvalidInput(format!("非法的 telegram chat_id: {chat}")))?;
        // Telegram 单条消息上限 4096，截断过长回复。
        let reply: String = text.chars().take(4000).collect();
        self.client
            .post(format!("{}/sendMessage", self.api))
            .json(&serde_json::json!({ "chat_id": chat_id, "text": reply }))
            .send()
            .await
            .map_err(|e| GanyuError::Http(e.to_string()))?;
        Ok(())
    }
}
