//! Discord 平台适配器（L3）：裸 `reqwest` 调 Discord REST API（无 SDK 依赖，与零依赖哲学一致）。
//!
//! 接收采用 REST 轮询（`GET /channels/{id}/messages?after=...`），复用 `PlatformAdapter::poll`
//! 的「拉取新消息」模型——与 Telegram 长轮询、HTTP 桥接挂起队列并列。
//! 发送用 `POST /channels/{id}/messages`。鉴权：`Authorization: Bot <token>`。
//!
//! 不回放历史：首次启动先「置位」游标到当前最新消息 id，仅处理其后新到的消息
//!（对齐 Telegram `getUpdates` 不回放行为）。
//!
//! 分页：`before` / `after` / `around` 三者**互斥**，且 `/messages` 固定「最新在前」返回，
//! 无法像 Slack 那样靠不透明 cursor 顺序翻页。故采用「自最新向过去走」——首页取最新一页，
//! 其后以本页最小 id 作 `before` 继续向更旧处翻，一旦越过游标（出现 id ≤ 游标）即追平停止。
//! 见 `fetch_channel` 的注释：直觉写法（游标取本页最小 id）会在积压超过一页时重复投递消息。
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

/// 单页拉取条数（`/messages` 的 limit 上限 100）。
const PAGE_LIMIT: usize = 100;

pub struct DiscordAdapter {
    client: reqwest::Client,
    token: String,
    /// 监控的频道 ID 列表（`[gateway] discord_channels`）。
    channels: Vec<String>,
    /// 每频道已处理的最新消息 snowflake id（游标）。未初始化时不回放历史。
    last_seen: Mutex<HashMap<String, String>>,
    /// 空闲（无新消息）轮询间隔，避免对 Discord REST 高频触发限流。
    idle_interval: Duration,
    /// REST 基址。抽成字段是为了让**网络层**可测（测试注入回环 mock 服务器）；
    /// 生产路径仍固定为 `DISCORD_API`。
    api_base: String,
}

impl DiscordAdapter {
    pub fn new(token: &str, channels: Vec<String>, idle_interval: Duration) -> Self {
        Self::with_base_url(token, channels, idle_interval, DISCORD_API)
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
        DiscordAdapter {
            client,
            token: token.to_string(),
            channels,
            last_seen: Mutex::new(HashMap::new()),
            idle_interval,
            api_base: api_base.to_string(),
        }
    }

    /// 置位游标：取频道当前最新消息 id（不回放历史）。失败返回 None（下轮重试）。
    async fn prime_newest_id(&self, ch: &str) -> Option<String> {
        let base = &self.api_base;
        let url = format!("{base}/channels/{ch}/messages?limit=1");
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

    /// 分页拉取某频道在游标 `after` 之后的全部新消息。
    ///
    /// **为什么不是「游标取本页最小 id」**：Discord 的 `before`/`after`/`around` 互斥，
    /// 且 `/messages` 固定「最新在前」。若把游标推进到本页最小 id（直觉上的「继续往后读」），
    /// 下一页 `after=<最小 id>` 拿到的仍是从最新往回数的同一批消息——每轮只前进 1 条却把
    /// 剩下的 99 条重复入队，积压 250 条时会投递近 500 条消息，agent 会重复回复同一句话。
    /// 正确走法是「自最新向过去走」：首页取最新一页，其后用本页最小 id 作 `before` 向更旧处翻，
    /// 遇到 id ≤ `after` 即说明已追平，停止。
    async fn fetch_channel(
        &self,
        ch: &str,
        after: Option<String>,
    ) -> GanyuResult<(Vec<InboundMessage>, u64)> {
        let base = &self.api_base;
        let cursor: u64 = after.as_ref().and_then(|s| s.parse().ok()).unwrap_or(0);
        let mut out: Vec<InboundMessage> = Vec::new();
        let mut global_max: u64 = cursor;
        let mut before: Option<String> = None;
        loop {
            let url = format!("{base}/channels/{ch}/messages");
            let mut req = self
                .client
                .get(&url)
                .query(&[("limit", PAGE_LIMIT.to_string())]);
            if let Some(b) = &before {
                req = req.query(&[("before", b.clone())]);
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
            // 翻页判定用**原始 id 列表**而非过滤后的消息：若本页 100 条里有 60 条是
            // bot/空文本而丢弃，按过滤后的条数判「不足一页」会提前收尾，漏掉更旧的新消息。
            let ids = page_message_ids(&parsed);
            if ids.is_empty() {
                break;
            }
            let mut reached_seen = false;
            let mut min_new = u64::MAX;
            for id in &ids {
                if *id <= cursor {
                    // 已越过游标：该条及其后更旧的都已处理过。
                    reached_seen = true;
                    continue;
                }
                if *id < min_new {
                    min_new = *id;
                }
            }
            for (id, msg) in parse_discord_messages(&parsed) {
                if id <= cursor {
                    continue;
                }
                out.push(msg);
                if id > global_max {
                    global_max = id;
                }
            }
            // 本页不足一页 → 频道内已无更旧消息；越过游标 → 已追平。
            if reached_seen || ids.len() < PAGE_LIMIT || min_new == u64::MAX {
                break;
            }
            before = Some(min_new.to_string());
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
        let base = &self.api_base;
        let reply: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
        self.client
            .post(format!("{base}/channels/{chat}/messages"))
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

/// 取响应数组里的**原始** snowflake id（最新在前），供分页游标判定使用。
///
/// 与 `parse_discord_messages` 分离：分页必须看服务端返回的真实条数，
/// 不能看过滤后的条数（bot/空文本被丢弃会让「不足一页」误判）。
fn page_message_ids(value: &serde_json::Value) -> Vec<u64> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|m| m["id"].as_str()?.parse::<u64>().ok())
        .collect()
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
    use std::collections::HashSet;

    use crate::gateway::mock_http;

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

    /// 分页游标只看服务端原始 id：bot / 空文本被过滤不应影响「不足一页」的判定。
    #[test]
    fn page_message_ids_reads_raw_ids_in_order() {
        let json = serde_json::json!([
            { "id": "30", "channel_id": "9", "author": { "username": "a", "bot": true }, "content": "x" },
            { "id": "20", "channel_id": "9", "author": { "username": "b" }, "content": "" },
            { "id": "10", "channel_id": "9", "author": { "username": "c" }, "content": "hi" }
        ]);
        assert_eq!(page_message_ids(&json), vec![30, 20, 10]);
        assert_eq!(
            parse_discord_messages(&json).len(),
            1,
            "过滤后只剩 1 条，但分页仍按 3 条计"
        );
    }

    // —— 以下为**网络层**测试：指向回环 mock 服务器，覆盖此前只有解析纯函数覆盖不到的
    //    真实 HTTP 往返（置位不回放、游标推进、分页终止、错误自愈、send 截断）。

    const CH: &str = "9";

    /// 模拟 Discord `/messages` 服务端行为：返回「最新在前」的一页 id。
    /// `all` 为频道内全部消息 id（升序），`before` 取严格小于它的最新一页。
    fn server_page(all: &[u64], before: Option<u64>, limit: usize) -> Vec<u64> {
        let mut ids: Vec<u64> = all
            .iter()
            .copied()
            .filter(|id| before.is_none_or(|b| *id < b))
            .collect();
        ids.sort_unstable_by(|a, b| b.cmp(a));
        ids.truncate(limit);
        ids
    }

    /// 构造 `/messages` 响应体（id 降序即「最新在前」）。
    fn discord_page(ids: &[u64]) -> String {
        let arr: Vec<serde_json::Value> = ids
            .iter()
            .map(|id| {
                serde_json::json!({
                    "id": id.to_string(),
                    "channel_id": CH,
                    "author": { "username": format!("u{id}") },
                    "content": format!("m{id}"),
                })
            })
            .collect();
        serde_json::to_string(&arr).expect("序列化消息页失败")
    }

    fn adapter(base_url: &str) -> DiscordAdapter {
        // idle_interval 取 0：无新消息时的节流休眠不应拖慢测试。
        DiscordAdapter::with_base_url(
            "tok",
            vec![CH.to_string()],
            Duration::from_millis(0),
            base_url,
        )
    }

    /// 首轮只置位游标，不回放历史。
    #[tokio::test]
    async fn prime_sets_cursor_without_replaying_history() {
        let server = mock_http::spawn(vec![discord_page(&[500])]).await;
        let a = adapter(&server.base_url);

        assert!(a.poll().await.expect("poll 不应失败").is_empty());
        assert_eq!(
            a.last_seen.lock().await.get(CH).map(String::as_str),
            Some("500")
        );
        let reqs = server.recorded().await;
        assert_eq!(reqs.len(), 1);
        assert!(
            reqs[0]
                .head
                .starts_with(&format!("GET /channels/{CH}/messages?limit=1 ")),
            "实际请求: {}",
            reqs[0].head
        );
    }

    /// 拉取新消息并推进游标；同一批消息不会在下一轮重复投递。
    #[tokio::test]
    async fn poll_returns_new_messages_and_advances_cursor() {
        let all: Vec<u64> = (1..=502).collect();
        let server = mock_http::spawn(vec![
            discord_page(&server_page(&all[..500], None, 1)), // 置位：最新为 500
            discord_page(&server_page(&all, None, PAGE_LIMIT)), // 新到 501 / 502
            discord_page(&server_page(&all, None, PAGE_LIMIT)), // 再轮询：无更新
        ])
        .await;
        let a = adapter(&server.base_url);

        assert!(a.poll().await.expect("poll 不应失败").is_empty());
        let msgs = a.poll().await.expect("poll 不应失败");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text, "m502", "Discord 最新在前，应先投递最新一条");
        assert_eq!(msgs[1].text, "m501");
        assert_eq!(msgs[0].chat_id, CH);
        assert_eq!(
            a.last_seen.lock().await.get(CH).map(String::as_str),
            Some("502")
        );
        assert!(
            a.poll().await.expect("poll 不应失败").is_empty(),
            "同一批消息不得重复投递"
        );
    }

    /// 积压超过一页时的分页走法：逐条投递且不重复。
    ///
    /// 这是 v0.1.24 的回归测试。旧实现把游标推进到「本页最小 id」后继续用 `after` 翻页，
    /// 而 Discord 固定「最新在前」，下一页拿到的仍是同一批消息——200 条积压会投递 199 条
    /// 含 99 条重复。正确走法是向更旧处翻（`before`），越过游标才停止。
    #[tokio::test]
    async fn drains_backlog_larger_than_one_page_without_duplicates() {
        let at_prime: Vec<u64> = (1..=500).collect();
        let later: Vec<u64> = (1..=700).collect(); // 置位后又到 200 条新消息
        let mut responses = vec![discord_page(&server_page(&at_prime, None, 1))];
        let mut before = None;
        loop {
            let page = server_page(&later, before, PAGE_LIMIT);
            let min = *page.iter().min().expect("页内至少一条");
            responses.push(discord_page(&page));
            if min <= 500 {
                break; // 该页已越过游标，适配器应在此停止
            }
            before = Some(min);
        }
        let fetch_pages = responses.len() - 1;
        let server = mock_http::spawn(responses).await;
        let a = adapter(&server.base_url);

        assert!(a.poll().await.expect("poll 不应失败").is_empty());
        let msgs = a.poll().await.expect("poll 不应失败");
        assert_eq!(msgs.len(), 200, "积压 200 条应逐条投递，不多不少");
        let unique: HashSet<&str> = msgs.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(unique.len(), 200, "出现重复投递：分页游标走法有误");
        assert_eq!(
            a.last_seen.lock().await.get(CH).map(String::as_str),
            Some("700")
        );
        assert_eq!(
            server.recorded().await.len(),
            fetch_pages + 1,
            "请求数应恰好等于「置位 + 各分页」，不得多打空转请求"
        );
    }

    /// 服务端返回错误时自愈为空结果（不向上抛错、不 panic），且游标不回退。
    /// release 配置为 `panic = "abort"`，适配器一旦panic会带走整个常驻网关进程。
    #[tokio::test]
    async fn fetch_error_is_survivable_and_keeps_cursor() {
        let all: Vec<u64> = (1..=500).collect();
        let server = mock_http::spawn_with_statuses(vec![
            (200, discord_page(&server_page(&all, None, 1))),
            (500, "boom".to_string()),
        ])
        .await;
        let a = adapter(&server.base_url);

        assert!(a.poll().await.expect("poll 不应失败").is_empty());
        let msgs = a.poll().await.expect("HTTP 500 应自愈为空结果而非报错");
        assert!(msgs.is_empty());
        assert_eq!(
            a.last_seen.lock().await.get(CH).map(String::as_str),
            Some("500"),
            "失败不得回退游标，否则会重放历史"
        );
    }

    /// 发送：命中 `/channels/{id}/messages`，带 Bot 鉴权，超长文本截断到 1900。
    #[tokio::test]
    async fn send_posts_truncated_content_with_bot_auth() {
        let server = mock_http::spawn(vec!["{}".to_string()]).await;
        let a = adapter(&server.base_url);

        a.send(CH, &"x".repeat(2500)).await.expect("send 不应失败");

        let reqs = server.recorded().await;
        assert_eq!(reqs.len(), 1);
        assert!(
            reqs[0]
                .head
                .starts_with(&format!("POST /channels/{CH}/messages ")),
            "实际请求: {}",
            reqs[0].head
        );
        let head = reqs[0].head.to_ascii_lowercase();
        assert!(
            head.contains("authorization: bot tok"),
            "实际请求头: {head}"
        );
        let body: serde_json::Value = serde_json::from_str(&reqs[0].body).expect("请求体应为 JSON");
        assert_eq!(
            body["content"].as_str().unwrap().chars().count(),
            MAX_MESSAGE_CHARS
        );
    }
}
