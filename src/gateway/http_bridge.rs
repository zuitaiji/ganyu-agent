//! HTTP Webhook 桥接适配器（L3）：axum 起本地端点，供 OpenClaw / 任意平台转发消息到 ganyu。
//!
//! 模型：请求/响应。平台（或 OpenClaw 桥接）`POST /message` 投递
//! `{chat_id?, user, text}`，handler 生成 request_id 入队并挂起等待 agent 回复，
//! 回复经 `send()` 通过 oneshot 回送 HTTP 响应。零平台 SDK 依赖，最贴近 OpenClaw 式网关。

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use tokio::sync::oneshot;

use crate::error::GanyuError;
use crate::gateway::{InboundMessage, PlatformAdapter};
use crate::GanyuResult;

/// 共享状态：队列（handler 入队 / adapter 出队）+ 挂起等待回复的 oneshot 表。
type Shared = (
    std::sync::Arc<Mutex<VecDeque<InboundMessage>>>,
    std::sync::Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
);

pub struct HttpBridge {
    #[allow(dead_code)]
    bind: String,
    queue: std::sync::Arc<Mutex<VecDeque<InboundMessage>>>,
    pending: std::sync::Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
    /// 后台 HTTP server 任务句柄（进程生命周期内常驻；保留以持有任务，避免被提前 drop）。
    #[allow(dead_code)]
    handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl HttpBridge {
    pub async fn new(bind: &str) -> GanyuResult<Self> {
        let queue = std::sync::Arc::new(Mutex::new(VecDeque::new()));
        let pending = std::sync::Arc::new(Mutex::new(HashMap::new()));
        let state: Shared = (queue.clone(), pending.clone());
        let app = Router::new()
            .route("/message", post(handle_message))
            .with_state(state);
        let addr: SocketAddr = bind
            .parse()
            .map_err(|e| GanyuError::InvalidInput(format!("非法绑定地址 {bind}: {e}")))?;
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
            handle: Mutex::new(Some(handle)),
        })
    }
}

/// `POST /message`：投递一条消息，挂起等待 agent 回复（120s 超时）。
async fn handle_message(
    State((queue, pending)): State<Shared>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let user = body["user"].as_str().unwrap_or("user").to_string();
    let text = body["text"].as_str().unwrap_or("").to_string();
    if text.trim().is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "empty text" }));
    }
    // request_id 同时用作 InboundMessage.chat_id，使 send() 能定位挂起的 oneshot。
    let req_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    pending.lock().unwrap().insert(req_id.clone(), tx);
    queue.lock().unwrap().push_back(InboundMessage {
        chat_id: req_id.clone(),
        user,
        text,
    });
    match tokio::time::timeout(Duration::from_secs(120), rx).await {
        Ok(Ok(reply)) => Json(serde_json::json!({ "ok": true, "reply": reply })),
        Ok(Err(_)) => Json(serde_json::json!({ "ok": false, "error": "agent dropped" })),
        Err(_) => Json(serde_json::json!({ "ok": false, "error": "timeout" })),
    }
}

#[async_trait]
impl PlatformAdapter for HttpBridge {
    fn name(&self) -> &str {
        "http"
    }

    async fn poll(&self) -> GanyuResult<Vec<InboundMessage>> {
        let mut q = self.queue.lock().unwrap();
        let msgs: Vec<InboundMessage> = q.drain(..).collect();
        Ok(msgs)
    }

    async fn send(&self, chat: &str, text: &str) -> GanyuResult<()> {
        // 把回复回送给对应 request_id 上挂起的 HTTP 请求。
        if let Some(tx) = self.pending.lock().unwrap().remove(chat) {
            let _ = tx.send(text.to_string());
        }
        Ok(())
    }
}
