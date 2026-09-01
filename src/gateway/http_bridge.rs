//! HTTP Webhook 桥接适配器（L3）：axum 起本地端点，供 OpenClaw / 任意平台转发消息到 ganyu。
//!
//! 模型：请求/响应。平台（或 OpenClaw 桥接）`POST /message` 投递
//! `{user, text}`，handler 生成 request_id 入队并挂起等待 agent 回复，
//! 回复经 `send()` 通过 oneshot 回送 HTTP 响应。零平台 SDK 依赖，最贴近 OpenClaw 式网关。
//!
//! 安全模型（fail-closed）：
//! - 绑定**非回环地址**时**必须**配置 token，否则拒绝启动——agent 具备 shell / 文件工具权限，
//!   无鉴权暴露等同于把本机执行权交给网络上可达的任何人。
//! - 配置 token 后，请求须携带 `Authorization: Bearer <token>`，否则 401。
//! - 请求体上限 64 KiB（消息文本远小于此），超限由 axum 直接返回 413。
//!
//! 并发模型：队列与挂起表用 `tokio::sync::Mutex`（异步锁，无 poison）。
//! 这与 `profile.release` 的 `panic = "abort"` 直接相关——`std::sync::Mutex` 在
//! 持锁线程 panic 后会 poison，后续 `.lock().unwrap()` 触发 panic 即整体 abort，
//! 常驻的网关进程会连同所有平台适配器一起退出。

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use tokio::sync::{oneshot, Mutex};

use crate::error::GanyuError;
use crate::gateway::{InboundMessage, PlatformAdapter};
use crate::GanyuResult;

/// 单条请求体上限（字节）：消息文本远小于此，用于阻断大 body 造成的内存 / DoS 面。
const MAX_BODY_BYTES: usize = 64 * 1024;

/// 共享状态：入队队列 + 挂起回复的 oneshot 表 + 鉴权 token（`None` = 仅回环免鉴权）。
type Shared = (
    Arc<Mutex<VecDeque<InboundMessage>>>,
    Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
    Option<String>,
);

pub struct HttpBridge {
    #[allow(dead_code)]
    bind: String,
    queue: Arc<Mutex<VecDeque<InboundMessage>>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
    /// 后台 HTTP server 任务句柄（进程生命周期内常驻；持有可能被 drop 导致服务提前终止）。
    #[allow(dead_code)]
    handle: tokio::task::JoinHandle<()>,
}

impl HttpBridge {
    /// 启动桥接端点。
    ///
    /// `token` 为 `None` 时**仅允许**绑定回环地址；非回环绑定直接拒绝（fail-closed），
    /// 避免无鉴权的 agent 端点暴露到局域网 / 公网。
    pub async fn new(bind: &str, token: Option<String>) -> GanyuResult<Self> {
        let addr: SocketAddr = bind
            .parse()
            .map_err(|e| GanyuError::InvalidInput(format!("非法绑定地址 {bind}: {e}")))?;
        if token.is_none() && !addr.ip().is_loopback() {
            return Err(GanyuError::InvalidInput(format!(
                "HTTP 桥接绑定在非回环地址 {addr} 但未配置鉴权 token：拒绝启动。\
                 请设置 GANYU_HTTP_TOKEN（或配置文件 [gateway] http_token），或改为绑定 127.0.0.1"
            )));
        }
        let queue: Arc<Mutex<VecDeque<InboundMessage>>> = Arc::new(Mutex::new(VecDeque::new()));
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let state: Shared = (queue.clone(), pending.clone(), token);
        let app = Router::new()
            .route("/message", post(handle_message))
            .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
            .with_state(state);
        let handle = tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[gateway:http] 绑定 {addr} 失败: {e}");
                    return;
                }
            };
            println!("[gateway:http] 桥接端点已启动: http://{addr}/message (POST)");
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("[gateway:http] 服务异常: {e}");
            }
        });
        Ok(HttpBridge {
            bind: bind.to_string(),
            queue,
            pending,
            handle,
        })
    }
}

/// 校验 `Authorization: Bearer <token>`；未配置 token 时直接放行（仅回环可达）。
fn authorized(headers: &HeaderMap, token: Option<&str>) -> bool {
    let Some(expected) = token else { return true };
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|got| got == expected)
}

/// `POST /message`：投递一条消息，挂起等待 agent 回复（120s 超时）。
async fn handle_message(
    State((queue, pending, token)): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !authorized(&headers, token.as_deref()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "ok": false, "error": "unauthorized" })),
        );
    }
    let user = body["user"].as_str().unwrap_or("user").to_string();
    let text = body["text"].as_str().unwrap_or("").to_string();
    if text.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": "empty text" })),
        );
    }
    // request_id 同时用作 InboundMessage.chat_id，使 send() 能定位挂起的 oneshot。
    let req_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    pending.lock().await.insert(req_id.clone(), tx);
    queue.lock().await.push_back(InboundMessage {
        chat_id: req_id.clone(),
        user,
        text,
    });
    match tokio::time::timeout(Duration::from_secs(120), rx).await {
        Ok(Ok(reply)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "reply": reply })),
        ),
        Ok(Err(_)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "ok": false, "error": "agent dropped" })),
        ),
        Err(_) => {
            // 超时后客户端已放弃：清理挂起项，避免 pending 表无界增长。
            pending.lock().await.remove(&req_id);
            (
                StatusCode::GATEWAY_TIMEOUT,
                Json(serde_json::json!({ "ok": false, "error": "timeout" })),
            )
        }
    }
}

#[async_trait]
impl PlatformAdapter for HttpBridge {
    fn name(&self) -> &str {
        "http"
    }

    async fn poll(&self) -> GanyuResult<Vec<InboundMessage>> {
        let mut q = self.queue.lock().await;
        Ok(q.drain(..).collect())
    }

    async fn send(&self, chat: &str, text: &str) -> GanyuResult<()> {
        // 把回复回送给对应 request_id 上挂起的 HTTP 请求。
        if let Some(tx) = self.pending.lock().await.remove(chat) {
            let _ = tx.send(text.to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未配置 token 时放行（仅回环场景可达，由 `new()` 保证）。
    #[test]
    fn no_token_allows_request() {
        let h = HeaderMap::new();
        assert!(authorized(&h, None));
    }

    #[test]
    fn token_missing_or_wrong_is_rejected() {
        let h = HeaderMap::new();
        assert!(!authorized(&h, Some("secret")));

        let mut wrong = HeaderMap::new();
        wrong.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer nope".parse().unwrap(),
        );
        assert!(!authorized(&wrong, Some("secret")));

        // 缺少 Bearer 前缀同样拒绝。
        let mut no_prefix = HeaderMap::new();
        no_prefix.insert(axum::http::header::AUTHORIZATION, "secret".parse().unwrap());
        assert!(!authorized(&no_prefix, Some("secret")));
    }

    #[test]
    fn valid_bearer_token_passes() {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer secret".parse().unwrap(),
        );
        assert!(authorized(&h, Some("secret")));
    }

    /// 安全不变量：非回环绑定且无 token → 拒绝启动（fail-closed）。
    #[tokio::test]
    async fn non_loopback_without_token_refuses_to_start() {
        let e = HttpBridge::new("0.0.0.0:0", None).await;
        assert!(e.is_err(), "非回环 + 无 token 应当拒绝启动");
    }
}
