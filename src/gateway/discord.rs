//! Discord 平台适配器（L3）：裸 `reqwest` 调 Discord REST API（无 SDK 依赖，与零依赖哲学一致）。
//!
//! 接收采用 REST 轮询（`GET /channels/{id}/messages?after=...`），复用 `PlatformAdapter::poll`
//! 的「拉取新消息」模型——与 Telegram 长轮询、HTTP 桥接挂起队列并列。
//! 发送用 `POST /channels/{id}/messages`。鉴权：`Authorization: Bot <token>`。
//!
//! 不回放历史：首次启动先「置位」游标到当前最新消息 id，仅处理其后新到的消息
//!（对齐 Telegram `getUpdates` 不回放行为）。分页 `after` 走最旧方向确保高吞吐频道不丢消息。
//! 跳过 bot / webhook 消息（`author.bot` 或 `webhook_id`）——本适配器自己的回复同样会进入
//! 频道历史，不过滤会形成「回复自己 → 再读回 → 再回复」的自激循环；与 SlackAdapter 的
//! `bot_id` 过滤是同一取舍（见技术规格 §6）。
//! 失败时打印并退避后返回空（自愈），不向上抛错打断网关。

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::Mutex;

use async_trait::async_trait;

use crate::error::GanyuError;
use crate::gateway::{InboundMessage, PlatformAdapter};
use crate::GanyuResult;

/// Discord REST API 基址（v10）。
const DISCORD_API: &str = "https://discord.com/api/v10";

/// 单条消息文本上限（Discord 为 2000；留余量避免越界）。
const MAX_MESSAGE_CHARS: usize = 1900;

pub struct DiscordAdapter {
    client: reqwest::Client,
    token: String,
    /// 监控的频道 ID 列表（`[gateway] discord_channels`）。
    channels: Vec<String>,
    /// 每频道已处理的最新消息 snowflake id（游标）。未初始化时不回放历史。
    last_seen: Mutex<HashMap<String, String>>,
    /// 空闲（无新消息）轮询间隔，避免对 Discord REST 高频触发限流。
    idle_interval: Duration,
}

impl DiscordAdapter {
    pub fn new(token: &str, channels: Vec<String>, idle_interval: Duration) -> Self {
        // 构建仅在 TLS 后端初始化失败时出错。降级为默认客户端而非 panic——
        // release 配置为 panic=abort，一次 panic 会让整个常驻网关进程（含其他平台）退出。
        let client = reqwest::Client::builder()
            .user_agent("ganyu-gateway")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        DiscordAdapter {
            client,
            token: token.to_string(),
            channels,
            last_seen: Mutex::new(HashMap::new()),
            idle_interval,
        }
    }

    /// 置位游标：取频道当前最新消息 id（不回放历史）。失败返回 None（下轮重试）。
    async fn prime_newest_id(&self, ch: &str) -> Option<String> {
        let url = format!("{DISCORD_API}/channels/{ch}/messages?limit=1");
        let resp = self
            .client
            .get(&url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bot {}", self.token),
            )
            .send()
            .await;
        let parsed: serde_json::Value = match resp {
            Ok(r) => r.json().await.ok()?,
            Err(_) => return None,
        };
        let arr = parsed.as_array()?;
        let first = arr.first()?;
        first["id"].as_str().map(|s| s.to_string())
    }

    /// 分页拉取某频道在 `after` 之后的所有新消息（走最旧方向，避免 >100 时丢消息）。
    async fn fetch_channel(
        &self,
        ch: &str,
        after: Option<String>,
    ) -> GanyuResult<(Vec<InboundMessage>, u64)> {
        let mut out: Vec<InboundMessage> = Vec::new();
        let mut global_max: u64 = after.as_ref().and_then(|s| s.parse().ok()).unwrap_or(0);
        let mut cursor = after;
        loop {
            let url = format!("{DISCORD_API}/channels/{ch}/messages");
            let mut req = self.client.get(&url).query(&[("limit", "100".to_string())]);
            if let Some(c) = &cursor {
                req = req.query(&[("after", c.clone())]);
            }
            let resp = req
                .header(
                    reqwest::header::AUTHORIZATION,
                    format!("Bot {}", self.token),
                )
                .send()
                .await;
            let parsed: serde_json::Value = match resp {
                Ok(r) => match r.error_for_status() {
                    Ok(r) => match r.json().await {
                        Ok(j) => j,
                        Err(e) => {
                            eprintln!("[gateway:discord] 解析 {ch} 响应失败: {e}，3s 后重试");
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            return Ok((out, global_max));
                        }
                    },
                    Err(e) => {
                        eprintln!("[gateway:discord] {ch} 返回错误: {e}，3s 后重试");
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        return Ok((out, global_max));
                    }
                },
                Err(e) => {
                    eprintln!("[gateway:discord] {ch} 请求错误: {e}，3s 后重试");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    return Ok((out, global_max));
                }
            };
            let pairs = parse_discord_messages(&parsed);
            if pairs.is_empty() {
                break;
            }
            let mut min_id = u64::MAX;
            for (id, msg) in &pairs {
                out.push(msg.clone());
                if *id > global_max {
                    global_max = *id;
                }
                if *id < min_id {
                    min_id = *id;
                }
            }
            if pairs.len() < 100 {
                break;
            }
            // 本页满 100，还有更旧的新消息：游标走向最旧方向继续翻页。
            cursor = Some(min_id.to_string());
        }
        Ok((out, global_max))
    }
}

#[async_trait]
impl PlatformAdapter for DiscordAdapter {
    fn name(&self) -> &str {
        "discord"
    }

    async fn poll(&self) -> GanyuResult<Vec<InboundMessage>> {
        let mut result: Vec<InboundMessage> = Vec::new();
        let mut any_new = false;
        for ch in &self.channels {
            // 未初始化：先置位游标到当前最新，不回放历史（对齐 Telegram getUpdates）。
            if !self.last_seen.lock().await.contains_key(ch) {
                if let Some(newest) = self.prime_newest_id(ch).await {
                    self.last_seen.lock().await.insert(ch.clone(), newest);
                }
                continue;
            }
            let after = self.last_seen.lock().await.get(ch).cloned();
            let (msgs, max_id) = self.fetch_channel(ch, after).await?;
            if !msgs.is_empty() {
                any_new = true;
                result.extend(msgs);
            }
            self.last_seen
                .lock()
                .await
                .insert(ch.clone(), max_id.to_string());
        }
        if !any_new {
            // 节流：无新消息时休眠，避免对 Discord REST 高频轮询触发限流。
            tokio::time::sleep(self.idle_interval).await;
        }
        Ok(result)
    }

    async fn send(&self, chat: &str, text: &str) -> GanyuResult<()> {
        // Discord 单条消息上限 2000，截断过长回复。
        let reply: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
        self.client
            .post(format!("{DISCORD_API}/channels/{chat}/messages"))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bot {}", self.token),
            )
            .json(&serde_json::json!({ "content": reply }))
            .send()
            .await
            .map_err(|e| GanyuError::Http(e.to_string()))?;
        Ok(())
    }
}

/// 解析 Discord `/messages` 响应数组为 (snowflake id, InboundMessage) 列表（纯函数，便于单测）。
fn parse_discord_messages(value: &serde_json::Value) -> Vec<(u64, InboundMessage)> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for m in arr {
        let Some(id_str) = m["id"].as_str() else {
            continue;
        };
        let Ok(id) = id_str.parse::<u64>() else {
            continue;
        };
        let chat_id = m["channel_id"].as_str().unwrap_or("").to_string();
        if chat_id.is_empty() {
            continue;
        }
        // 跳过 bot / webhook 消息：本适配器自己的回复同样会进入频道历史
        //（经 Bot token 发出的消息其 author 带 bot=true），不跳过会形成
        //「回复自己 → 再读回 → 再回复」的自激循环。与 SlackAdapter 的 bot_id
        // 过滤是同一取舍（见技术规格 §6），Discord 侧标识为 author.bot / webhook_id。
        if m["author"]["bot"] == serde_json::Value::Bool(true) || m.get("webhook_id").is_some() {
            continue;
        }
        let user = m["author"]["username"]
            .as_str()
            .or_else(|| m["author"]["global_name"].as_str())
            .unwrap_or("user")
            .to_string();
        let text = m["content"].as_str().unwrap_or("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        out.push((
            id,
            InboundMessage {
                chat_id,
                user,
                text,
            },
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解析映射字段：id / channel_id / author.username / content。
    #[test]
    fn parse_discord_messages_maps_fields() {
        let json = serde_json::json!([
            {
                "id": "10",
                "channel_id": "999",
                "author": { "username": "alice", "global_name": "Alice" },
                "content": "hello"
            },
            {
                "id": "11",
                "channel_id": "999",
                "author": { "username": "bob" },
                "content": "world"
            }
        ]);
        let pairs = parse_discord_messages(&json);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, 10);
        assert_eq!(pairs[0].1.chat_id, "999");
        assert_eq!(pairs[0].1.user, "alice");
        assert_eq!(pairs[0].1.text, "hello");
        assert_eq!(pairs[1].0, 11);
        assert_eq!(pairs[1].1.user, "bob");
    }

    /// 空 content 的消息被跳过（Discord 编辑/嵌入等无文本消息不驱动 agent）。
    #[test]
    fn parse_skips_empty_content() {
        let json = serde_json::json!([
            { "id": "1", "channel_id": "5", "author": { "username": "x" }, "content": "" },
            { "id": "2", "channel_id": "5", "author": { "username": "y" }, "content": "  " }
        ]);
        assert!(parse_discord_messages(&json).is_empty());
    }

    /// 跳过 bot / webhook 消息，但保留真人消息：本适配器自己的回复带 `bot=true`，
    /// 不过滤会自激循环；同时必须确认 `bot=false` 的真人消息不被误杀。
    #[test]
    fn parse_skips_bot_and_webhook() {
        let json = serde_json::json!([
            { "id": "10", "channel_id": "5", "author": { "username": "self", "bot": true }, "content": "echo" },
            { "id": "11", "channel_id": "5", "author": { "username": "hook" }, "webhook_id": "9", "content": "via webhook" },
            { "id": "12", "channel_id": "5", "author": { "username": "human", "bot": false }, "content": "real" }
        ]);
        let pairs = parse_discord_messages(&json);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, 12);
        assert_eq!(pairs[0].1.user, "human");
        assert_eq!(pairs[0].1.text, "real");
    }

    /// 构造器与名称不变量。
    #[test]
    fn new_builds_with_name() {
        let a = DiscordAdapter::new("tok", vec!["123".into()], Duration::from_secs(2));
        assert_eq!(a.name(), "discord");
    }
}
