//! Slack 平台适配器（L3）：裸 `reqwest` 调 Slack Web API（无 SDK 依赖，与零依赖哲学一致）。
//!
//! 接收采用 REST 轮询（`GET /conversations.history?channel=...&oldest=<ts>`），复用
//! `PlatformAdapter::poll` 的「拉取新消息」模型——与 Telegram 长轮询、Discord 轮询并列。
//! 发送用 `POST /chat.postMessage`。鉴权：`Authorization: Bearer <token>`。
//!
//! 不回放历史：首次启动先「置位」游标到当前最新消息 ts，仅处理其后新到的消息
//!（对齐 Telegram `getUpdates` / Discord 不回放行为）。分页跟随 `response_metadata.next_cursor`
//! 直至 `has_more=false`，确保高吞吐频道不丢消息。
//!
//! 与 Discord 的两点差异：
//! 1. `conversations.history` 的响应**不含**每条消息的频道字段，故 `chat_id` 由查询时的
//!    频道 ID 注入——否则回复无法路由回原频道。
//! 2. 跳过 `bot_id` 消息：本适配器自己的回复同样会进入频道历史，不跳过会形成
//!    「回复自己 → 再读回 → 再回复」的自激循环。
//!
//! 失败时打印并退避后返回空（自愈），不向上抛错打断网关。

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::Mutex;

use async_trait::async_trait;

use crate::error::GanyuError;
use crate::gateway::{InboundMessage, PlatformAdapter};
use crate::GanyuResult;

/// Slack Web API 基址。
const SLACK_API: &str = "https://slack.com/api";

/// 单条消息文本上限。Slack 理论上限约 40000 字符，此处保守截断：
/// 远低于上限故不会触发 API 拒绝，同时避免超长回复压垮频道可读性。
const MAX_MESSAGE_CHARS: usize = 3900;

/// 单页拉取条数（`conversations.history` 上限 200）。
const PAGE_LIMIT: u32 = 200;

pub struct SlackAdapter {
    client: reqwest::Client,
    token: String,
    /// 监控的频道 ID 列表（`[gateway] slack_channels`）。
    channels: Vec<String>,
    /// 每频道已处理的最新消息 ts（游标）。未初始化时不回放历史。
    last_seen: Mutex<HashMap<String, String>>,
    /// 空闲（无新消息）轮询间隔，避免对 Slack Web API 高频触发限流。
    idle_interval: Duration,
    /// REST 基址。抽成字段是为了让**网络层**可测（测试注入回环 mock 服务器）；
    /// 生产路径仍固定为 `SLACK_API`。
    api_base: String,
}

impl SlackAdapter {
    pub fn new(token: &str, channels: Vec<String>, idle_interval: Duration) -> Self {
        Self::with_base_url(token, channels, idle_interval, SLACK_API)
    }

    fn with_base_url(
        token: &str,
        channels: Vec<String>,
        idle_interval: Duration,
        api_base: &str,
    ) -> Self {
        // 构建仅在 TLS 后端初始化失败时出错。降级为默认客户端而非 panic——
        // release 配置为 panic=abort，一次 panic 会让整个常驻网关进程（含其他平台）退出。
        // no_proxy：测试指向 127.0.0.1 的 mock 服务器，必须绕开环境代理，否则 CI 上会串到
        // 外部代理而连不通（system-proxy 特性会读取 HTTP_PROXY）。
        let client = reqwest::Client::builder()
            .user_agent("ganyu-gateway")
            .no_proxy()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        SlackAdapter {
            client,
            token: token.to_string(),
            channels,
            last_seen: Mutex::new(HashMap::new()),
            idle_interval,
            api_base: api_base.to_string(),
        }
    }

    /// 置位游标：取频道当前最新消息 ts（不回放历史）。失败返回 None（下轮重试）。
    async fn prime_latest_ts(&self, ch: &str) -> Option<String> {
        let page = self.history_page(ch, None, None).await?;
        // Slack 按最新在前返回：首条即当前最新。
        page.get("messages")?.as_array()?.first()?["ts"]
            .as_str()
            .map(|s| s.to_string())
    }

    /// 拉取单页 `conversations.history`。`oldest` 为游标（不含自身），`cursor` 为分页游标。
    async fn history_page(
        &self,
        ch: &str,
        oldest: Option<String>,
        cursor: Option<String>,
    ) -> Option<serde_json::Value> {
        let mut req = self
            .client
            .get(format!("{}/conversations.history", self.api_base))
            .query(&[("channel", ch.to_string())])
            .query(&[("limit", PAGE_LIMIT.to_string())]);
        if let Some(o) = oldest {
            req = req.query(&[("oldest", o)]);
        }
        if let Some(c) = cursor {
            req = req.query(&[("cursor", c)]);
        }
        let resp = req
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token),
            )
            .send()
            .await;
        let resp = match resp {
            Ok(r) => match r.error_for_status() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[gateway:slack] {ch} 返回错误: {e}，3s 后重试");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    return None;
                }
            },
            Err(e) => {
                eprintln!("[gateway:slack] {ch} 请求错误: {e}，3s 后重试");
                tokio::time::sleep(Duration::from_secs(3)).await;
                return None;
            }
        };
        let parsed: serde_json::Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                eprintln!("[gateway:slack] 解析 {ch} 响应失败: {e}，3s 后重试");
                tokio::time::sleep(Duration::from_secs(3)).await;
                return None;
            }
        };
        // Slack 的 API 错误返回 HTTP 200 + {"ok":false,"error":"invalid_auth"}，
        // 仅靠 error_for_status 判别不到，必须显式检查 ok 字段（否则会把错误体当空消息吞掉）。
        if parsed["ok"] != serde_json::Value::Bool(true) {
            let err = parsed["error"].as_str().unwrap_or("unknown");
            eprintln!("[gateway:slack] {ch} API 错误: {err}，3s 后重试");
            tokio::time::sleep(Duration::from_secs(3)).await;
            return None;
        }
        Some(parsed)
    }

    /// 分页拉取某频道在游标之后的全部新消息（跟随 next_cursor 直至 has_more=false）。
    async fn fetch_channel(
        &self,
        ch: &str,
        oldest: Option<String>,
    ) -> GanyuResult<(Vec<InboundMessage>, String)> {
        let mut out: Vec<InboundMessage> = Vec::new();
        // ts 必须原样保留作游标（"1503435956.000247" 转 f64 再转回会丢精度），
        // 故比较用 f64、存储用原始字符串。
        let mut max_ts_val: f64 = oldest
            .as_ref()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let mut max_ts_str: String = oldest.clone().unwrap_or_default();
        let mut cursor: Option<String> = None;
        loop {
            let Some(page) = self.history_page(ch, oldest.clone(), cursor.clone()).await else {
                break;
            };
            for (ts, msg) in parse_slack_messages(&page, ch) {
                out.push(msg);
                if let Ok(v) = ts.parse::<f64>() {
                    if v > max_ts_val {
                        max_ts_val = v;
                        max_ts_str = ts;
                    }
                }
            }
            let has_more = page["has_more"].as_bool().unwrap_or(false);
            let next = page
                .get("response_metadata")
                .and_then(|m| m["next_cursor"].as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            match next {
                Some(n) if has_more => cursor = Some(n),
                _ => break,
            }
        }
        Ok((out, max_ts_str))
    }
}

#[async_trait]
impl PlatformAdapter for SlackAdapter {
    fn name(&self) -> &str {
        "slack"
    }

    async fn poll(&self) -> GanyuResult<Vec<InboundMessage>> {
        let mut result: Vec<InboundMessage> = Vec::new();
        let mut any_new = false;
        for ch in &self.channels {
            // 未初始化：先置位游标到当前最新，不回放历史（对齐 Telegram getUpdates）。
            if !self.last_seen.lock().await.contains_key(ch) {
                if let Some(latest) = self.prime_latest_ts(ch).await {
                    self.last_seen.lock().await.insert(ch.clone(), latest);
                }
                continue;
            }
            let oldest = self.last_seen.lock().await.get(ch).cloned();
            let (msgs, max_ts) = self.fetch_channel(ch, oldest).await?;
            if !msgs.is_empty() {
                any_new = true;
                result.extend(msgs);
            }
            self.last_seen.lock().await.insert(ch.clone(), max_ts);
        }
        if !any_new {
            // 节流：无新消息时休眠，避免对 Slack Web API 高频轮询触发限流。
            tokio::time::sleep(self.idle_interval).await;
        }
        Ok(result)
    }

    async fn send(&self, chat: &str, text: &str) -> GanyuResult<()> {
        // 截断过长回复（见 MAX_MESSAGE_CHARS）。
        let reply: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
        self.client
            .post(format!("{}/chat.postMessage", self.api_base))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token),
            )
            .json(&serde_json::json!({ "channel": chat, "text": reply }))
            .send()
            .await
            .map_err(|e| GanyuError::Http(e.to_string()))?;
        Ok(())
    }
}

/// 解析 Slack `conversations.history` 响应为 (ts, InboundMessage) 列表（纯函数，便于单测）。
///
/// `channel` 由调用方注入：Slack 响应体不含每条消息的频道字段，而回复必须路由回原频道。
/// 跳过两类消息：空文本（编辑/附件等不驱动 agent）、带 `bot_id`（含本适配器自己的回复，防自激）。
fn parse_slack_messages(value: &serde_json::Value, channel: &str) -> Vec<(String, InboundMessage)> {
    let Some(arr) = value.get("messages").and_then(|m| m.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for m in arr {
        let Some(ts) = m["ts"].as_str() else {
            continue;
        };
        if m.get("bot_id").is_some() {
            continue;
        }
        let user = m["user"].as_str().unwrap_or("user").to_string();
        let text = m["text"].as_str().unwrap_or("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        out.push((
            ts.to_string(),
            InboundMessage {
                chat_id: channel.to_string(),
                user,
                text,
            },
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::gateway::mock_http;

    use super::*;

    /// 解析映射字段：ts / user / text，且 chat_id 由查询频道注入（Slack 响应体不含该字段）。
    #[test]
    fn parse_slack_messages_maps_fields() {
        let json = serde_json::json!({
            "ok": true,
            "messages": [
                { "ts": "1503435956.000247", "user": "U1", "text": "hello" },
                { "ts": "1503435957.000100", "user": "U2", "text": "world" }
            ]
        });
        let pairs = parse_slack_messages(&json, "C99");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, "1503435956.000247");
        assert_eq!(pairs[0].1.chat_id, "C99");
        assert_eq!(pairs[0].1.user, "U1");
        assert_eq!(pairs[0].1.text, "hello");
        assert_eq!(pairs[1].0, "1503435957.000100");
        assert_eq!(pairs[1].1.user, "U2");
    }

    /// 跳过 bot 消息与空文本：bot_id 用于阻断「回复自己 → 再读回」的自激循环。
    #[test]
    fn parse_skips_bot_and_empty() {
        let json = serde_json::json!({
            "ok": true,
            "messages": [
                { "ts": "1.1", "user": "U1", "text": "hi", "bot_id": "B1" },
                { "ts": "1.2", "user": "U2", "text": "" },
                { "ts": "1.3", "user": "U3", "text": "   " }
            ]
        });
        assert!(parse_slack_messages(&json, "C1").is_empty());
    }

    /// API 错误体（{"ok":false} 且无 messages）解析为空，不会把错误当成消息处理。
    #[test]
    fn parse_handles_error_payload() {
        let json = serde_json::json!({ "ok": false, "error": "missing_scope" });
        assert!(parse_slack_messages(&json, "C1").is_empty());
    }

    /// 构造器与名称不变量。
    #[test]
    fn new_builds_with_name() {
        let a = SlackAdapter::new("xoxb-tok", vec!["C1".into()], Duration::from_secs(2));
        assert_eq!(a.name(), "slack");
    }

    // —— 以下为**网络层**测试：指向回环 mock 服务器，覆盖此前只有解析纯函数覆盖不到的
    //    真实 HTTP 往返（置位不回放、游标推进、next_cursor 分页、ok:false 判别、send 截断）。

    const CH: &str = "C1";
    const TOKEN: &str = "xoxb-tok";

    /// 构造 `conversations.history` 响应体（Slack 返回「最新在前」）。
    fn slack_history(messages: &[(&str, &str)], has_more: bool, next_cursor: &str) -> String {
        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .map(|(ts, text)| {
                serde_json::json!({
                    "ts": ts.to_string(),
                    "user": format!("U{ts}"),
                    "text": text.to_string(),
                })
            })
            .collect();
        serde_json::json!({
            "ok": true,
            "messages": msgs,
            "has_more": has_more,
            "response_metadata": { "next_cursor": next_cursor }
        })
        .to_string()
    }

    fn adapter(base_url: &str) -> SlackAdapter {
        // idle_interval 取 0：无新消息时的节流休眠不应拖慢测试。
        SlackAdapter::with_base_url(
            TOKEN,
            vec![CH.to_string()],
            Duration::from_millis(0),
            base_url,
        )
    }

    /// 首轮只置位游标，不回放历史。
    #[tokio::test]
    async fn prime_sets_cursor_without_replaying_history() {
        let server = mock_http::spawn(vec![slack_history(
            &[("1503435956.000247", "m")],
            false,
            "",
        )])
        .await;
        let a = adapter(&server.base_url);

        assert!(a.poll().await.expect("poll 不应失败").is_empty());
        assert_eq!(
            a.last_seen.lock().await.get(CH).map(String::as_str),
            Some("1503435956.000247")
        );
        let reqs = server.recorded().await;
        assert_eq!(reqs.len(), 1);
        assert!(
            reqs[0].head.starts_with("GET /conversations.history?"),
            "实际请求: {}",
            reqs[0].head
        );
    }

    /// 拉取新消息并推进游标；同一批消息不会在下一轮重复投递。
    #[tokio::test]
    async fn poll_returns_new_messages_and_advances_cursor() {
        let server = mock_http::spawn(vec![
            slack_history(&[("500.000000", "m500")], false, ""), // 置位：最新 500
            slack_history(&[("502.000000", "m502"), ("501.000000", "m501")], false, ""), // 新到 501/502
            slack_history(&[], false, ""), // 再轮询：无更新
        ])
        .await;
        let a = adapter(&server.base_url);

        assert!(a.poll().await.expect("poll 不应失败").is_empty());
        let msgs = a.poll().await.expect("poll 不应失败");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text, "m502", "Slack 最新在前，应先投递最新一条");
        assert_eq!(msgs[1].text, "m501");
        assert_eq!(msgs[0].chat_id, CH);
        assert_eq!(
            a.last_seen.lock().await.get(CH).map(String::as_str),
            Some("502.000000")
        );
        assert!(
            a.poll().await.expect("poll 不应失败").is_empty(),
            "同一批消息不得重复投递"
        );
    }

    /// next_cursor 分页：高吞吐频道多页拉取，逐条投递且不重复，直至 has_more=false。
    #[tokio::test]
    async fn paginates_via_next_cursor_until_has_more_false() {
        let server = mock_http::spawn(vec![
            slack_history(&[("700.000000", "m700")], false, ""), // 置位：最新 700
            slack_history(
                &[("702.000000", "m702"), ("701.000000", "m701")],
                true,
                "cur1",
            ), // 页1
            slack_history(
                &[("704.000000", "m704"), ("703.000000", "m703")],
                true,
                "cur2",
            ), // 页2
            slack_history(&[("705.000000", "m705")], false, ""), // 页3：has_more=false
        ])
        .await;
        let a = adapter(&server.base_url);

        assert!(a.poll().await.expect("poll 不应失败").is_empty());
        let msgs = a.poll().await.expect("poll 不应失败");
        assert_eq!(msgs.len(), 5, "三页累计 5 条新消息，逐条投递");
        let ts: HashSet<&str> = msgs.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(ts.len(), 5, "出现重复投递：next_cursor 分页走法有误");
        assert_eq!(
            a.last_seen.lock().await.get(CH).map(String::as_str),
            Some("705.000000"),
            "游标应推进到最新一条的 ts"
        );
        assert_eq!(
            server.recorded().await.len(),
            4,
            "请求数应恰好等于「置位 + 各分页(3)」，不得多打空转请求"
        );
    }

    /// Slack 的签名坑：API 错误返回 HTTP 200 + {"ok":false}，必须显式判别，
    /// 否则会把错误体当空消息吞掉（不影响游标、不 panic）。
    #[tokio::test]
    async fn ok_false_is_detected_and_survives() {
        let server = mock_http::spawn(vec![
            slack_history(&[("500.000000", "m500")], false, ""), // 置位：500
            serde_json::json!({ "ok": false, "error": "invalid_auth" }).to_string(), // 错误体（HTTP 200）
        ])
        .await;
        let a = adapter(&server.base_url);

        assert!(a.poll().await.expect("poll 不应失败").is_empty());
        let msgs = a.poll().await.expect("ok:false 应自愈为空结果而非报错");
        assert!(msgs.is_empty());
        assert_eq!(
            a.last_seen.lock().await.get(CH).map(String::as_str),
            Some("500.000000"),
            "失败不得回退游标，否则会重放历史"
        );
    }

    /// 发送：命中 `POST /chat.postMessage`，带 Bearer 鉴权，超长文本截断到 3900。
    #[tokio::test]
    async fn send_posts_with_bearer_auth_and_truncates() {
        let server = mock_http::spawn(vec!["{}".to_string()]).await;
        let a = adapter(&server.base_url);

        a.send(CH, &"x".repeat(5000)).await.expect("send 不应失败");

        let reqs = server.recorded().await;
        assert_eq!(reqs.len(), 1);
        assert!(
            reqs[0].head.starts_with("POST /chat.postMessage"),
            "实际请求: {}",
            reqs[0].head
        );
        let head = reqs[0].head.to_ascii_lowercase();
        assert!(
            head.contains("authorization: bearer xoxb-tok"),
            "实际请求头: {head}"
        );
        let body: serde_json::Value = serde_json::from_str(&reqs[0].body).expect("请求体应为 JSON");
        assert_eq!(body["channel"].as_str(), Some(CH));
        assert_eq!(
            body["text"].as_str().unwrap().chars().count(),
            MAX_MESSAGE_CHARS
        );
    }
}
