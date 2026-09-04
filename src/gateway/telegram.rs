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
        Self::with_base_url(&format!("https://api.telegram.org/bot{token}"))
    }

    fn with_base_url(api_base: &str) -> Self {
        // 构建仅在 TLS 后端初始化失败时出错。降级为默认客户端而非 panic——
        // release 配置为 panic=abort，一次 panic 会让整个常驻网关进程（含其他平台）退出。
        // no_proxy：测试指向 127.0.0.1 的 mock 服务器，必须绕开环境代理，否则 CI 上会串到
        // 外部代理而连不通（system-proxy 特性会读取 HTTP_PROXY）。
        let client = reqwest::Client::builder()
            .user_agent("ganyu-gateway")
            .no_proxy()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        TelegramAdapter {
            client,
            api: api_base.to_string(),
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
            Ok(r) => match r.error_for_status() {
                Ok(r) => r
                    .json()
                    .await
                    .map_err(|e| GanyuError::Http(e.to_string()))?,
                Err(e) => {
                    // 服务端返回非 2xx（如 5xx）：退避后返回空（自愈），不向上抛错打断网关
                    // （对齐 Discord/Slack 的 `error_for_status` 错误处理）。`error_for_status`
                    // 失败必须在此吞掉而非 `?` 传播——否则 Telegram 侧一次事故会让本适配器
                    // 持续报错、常驻网关的 run_adapter 只能靠自身 2s 重试空转。
                    eprintln!("[gateway:telegram] getUpdates HTTP 错误: {e}，3s 后重试");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    return Ok(Vec::new());
                }
            },
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

#[cfg(test)]
mod tests {
    use crate::gateway::mock_http;

    use super::*;

    /// 构造 `getUpdates` 成功响应体：`{ "ok": true, "result": [...] }`。
    fn tg_ok(result: &[serde_json::Value]) -> String {
        serde_json::json!({ "ok": true, "result": result }).to_string()
    }

    /// 构造一条带文本的消息 update（Telegram `update_id` 与 `message_id` 各自独立，此处取同值即可）。
    fn tg_msg(update_id: u64, username: &str, chat_id: i64, text: &str) -> serde_json::Value {
        serde_json::json!({
            "update_id": update_id,
            "message": {
                "message_id": update_id,
                "from": { "username": username },
                "chat": { "id": chat_id },
                "text": text
            }
        })
    }

    fn adapter(base_url: &str) -> TelegramAdapter {
        TelegramAdapter::with_base_url(base_url)
    }

    /// 构造器与名称不变量。
    #[test]
    fn new_builds_with_name() {
        let a = TelegramAdapter::new("tok");
        assert_eq!(a.name(), "telegram");
    }

    // —— 以下为**网络层**测试：指向回环 mock 服务器，覆盖此前零覆盖的真实 HTTP 往返
    //    （offset 推进、不回放、对非文本 update 的 offset 越过、send 截断+chat_id 解析、
    //    HTTP 500 与 ok:false 的自愈）。Telegram 没有显式「置位」步骤——它靠 getUpdates
    //    的 `offset` 游标实现「不回放」：首轮 offset=0 取回服务端待处理队列，处理后 offset
    //    推到 `max(update_id)+1`，后续轮询即不再重放同一批消息。

    /// 首轮取回消息并推进 offset，次轮（offset 已越过）不再回放历史。
    #[tokio::test]
    async fn poll_returns_new_messages_and_advances_offset() {
        let server = mock_http::spawn(vec![
            tg_ok(&[
                tg_msg(100, "alice", -100, "hello"),
                tg_msg(101, "bob", -100, "world"),
            ]),
            tg_ok(&[]),
        ])
        .await;
        let a = adapter(&server.base_url);

        let msgs = a.poll().await.expect("poll 不应失败");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text, "hello");
        assert_eq!(msgs[0].user, "alice");
        assert_eq!(msgs[0].chat_id, "-100");
        assert_eq!(msgs[1].text, "world");

        assert!(
            a.poll().await.expect("poll 不应失败").is_empty(),
            "offset 推进后应不再回放历史消息"
        );

        assert_eq!(
            *a.offset.lock().await,
            102,
            "offset 应推进到最后处理 update_id + 1"
        );
        let reqs = server.recorded().await;
        assert_eq!(reqs.len(), 2);
        assert!(
            reqs[0].head.starts_with("GET /getUpdates?"),
            "实际请求: {}",
            reqs[0].head
        );
    }

    /// offset 必须越过**所有** update（含无文本 / 非 message 键者），而非只越过有效消息，
    /// 否则那些被跳过的 update 会在下一轮被重新取回（Telegram 的 offset 语义是按 update_id 确认）。
    #[tokio::test]
    async fn offset_advances_past_all_updates_without_replay() {
        let server = mock_http::spawn(vec![
            tg_ok(&[
                tg_msg(200, "alice", -100, "hi"), // 有效文本消息
                tg_msg(201, "bob", -100, ""),     // 空文本 → 跳过
                serde_json::json!({                // 非 message 键（edited_message）→ 无文本 → 跳过
                    "update_id": 202,
                    "edited_message": {
                        "message_id": 202,
                        "from": { "username": "carol" },
                        "chat": { "id": -100 },
                        "text": "x"
                    }
                }),
                tg_msg(203, "dave", -100, "   "), // 纯空白文本 → trim 后空 → 跳过
            ]),
            tg_ok(&[]),
        ])
        .await;
        let a = adapter(&server.base_url);

        let msgs = a.poll().await.expect("poll 不应失败");
        assert_eq!(msgs.len(), 1, "仅 1 条带有效文本的消息应被投递");
        assert_eq!(msgs[0].text, "hi");

        assert!(
            a.poll().await.expect("poll 不应失败").is_empty(),
            "offset 应越过全部 4 条 update（含无文本者），不重复投递"
        );
        assert_eq!(
            *a.offset.lock().await,
            204,
            "offset 必须推进到最后一个 update_id + 1，而非只到最后一条有效消息"
        );
        assert_eq!(server.recorded().await.len(), 2);
    }

    /// 发送：命中 `POST /sendMessage`，`chat_id` 解析为 i64，超长文本截断到 4000；
    /// 非法 chat_id 在发请求前即以 `InvalidInput` 失败（不发 HTTP）。
    #[tokio::test]
    async fn send_posts_to_sendmessage_and_truncates() {
        let server = mock_http::spawn(vec!["{}".to_string()]).await;
        let a = adapter(&server.base_url);

        a.send("-100", &"x".repeat(5000))
            .await
            .expect("send 不应失败");

        let reqs = server.recorded().await;
        assert_eq!(reqs.len(), 1);
        assert!(
            reqs[0].head.starts_with("POST /sendMessage"),
            "实际请求: {}",
            reqs[0].head
        );
        let body: serde_json::Value = serde_json::from_str(&reqs[0].body).expect("请求体应为 JSON");
        assert_eq!(body["chat_id"].as_i64(), Some(-100), "chat_id 须解析为 i64");
        assert_eq!(
            body["text"].as_str().unwrap().chars().count(),
            4000,
            "文本须截断到 4000（上限 4096 留余量）"
        );

        // 非法 chat_id：在发起 HTTP 前即失败，不向服务器打请求。
        assert!(
            a.send("not-a-number", "hi").await.is_err(),
            "非数字 chat_id 应返回 InvalidInput"
        );
        assert_eq!(
            server.recorded().await.len(),
            1,
            "非法 chat_id 不得产生额外请求"
        );
    }

    /// HTTP 500 自愈：返回空结果而非报错，且 offset 不回退（release 为 panic=abort，
    /// 一旦 panic 会带走整个常驻网关进程）。
    #[tokio::test]
    async fn http_error_is_survivable_and_keeps_offset() {
        let server = mock_http::spawn_with_statuses(vec![
            (200, tg_ok(&[tg_msg(100, "alice", -100, "hello")])), // 投递，offset → 101
            (500, "boom".to_string()),                            // HTTP 500 → 自愈
            (200, tg_ok(&[])),                                    // 恢复后空轮询
        ])
        .await;
        let a = adapter(&server.base_url);

        assert_eq!(a.poll().await.expect("首次 poll 不应失败").len(), 1);
        assert_eq!(*a.offset.lock().await, 101);

        let after_err = a
            .poll()
            .await
            .expect("HTTP 500 应自愈为空结果而非报错（3s 退避）");
        assert!(after_err.is_empty());
        assert_eq!(
            *a.offset.lock().await,
            101,
            "失败不得回退 offset，否则会重放历史"
        );
        assert!(
            a.poll().await.expect("恢复后 poll 不应失败").is_empty(),
            "恢复后仍能正常轮询"
        );
    }

    /// Telegram 的签名坑：协议层错误返回 HTTP 200 + `{"ok":false}`，必须显式判别，
    /// 否则会把错误体当空消息吞掉（不影响 offset、不 panic）。
    #[tokio::test]
    async fn ok_false_is_detected_and_survives() {
        let server = mock_http::spawn(vec![
            tg_ok(&[tg_msg(100, "alice", -100, "hello")]), // offset → 101
            serde_json::json!({ "ok": false, "error_code": 401, "description": "Unauthorized" })
                .to_string(), // HTTP 200 + ok:false
            tg_ok(&[]),
        ])
        .await;
        let a = adapter(&server.base_url);

        assert_eq!(a.poll().await.expect("首次 poll 不应失败").len(), 1);
        assert_eq!(*a.offset.lock().await, 101);

        let after = a
            .poll()
            .await
            .expect("ok:false 应自愈为空结果而非报错（3s 退避）");
        assert!(after.is_empty());
        assert_eq!(*a.offset.lock().await, 101, "失败不得回退 offset");
    }
}
