//! 测试专用：极简 mock HTTP 服务器（仅 `cfg(all(test, feature = "network"))` 下编译，零新增依赖）。
//!
//! 存在的理由：网关适配器的**网络层**（分页终止、游标推进、`ok:false` 判别、`send` 截断）
//! 此前完全无测试覆盖——已有单测只覆盖解析纯函数。真实 Discord/Slack token 无法在 CI 中获取，
//! 故用回环 mock 服务器做真实 HTTP 往返验证，而不是只验证「我能解析我手写的 JSON」。
//!
//! 刻意不引入 wiremock/httpmock 等 dev 依赖：本项目对依赖体积敏感，且所需仅是
//! 「按序返回固定响应 + 记录请求」这几十行能力。

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// 捕获到的一次请求：`head` 含请求行与全部请求头（用于断言路径与查询串），`body` 为正文。
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    pub head: String,
    pub body: String,
}

/// 回环 mock 服务器：按连接到达顺序依次返回预设响应，并记录收到的请求。
pub struct MockServer {
    /// 形如 `http://127.0.0.1:PORT`，供适配器作为 base_url 注入。
    pub base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

impl MockServer {
    /// 按到达顺序返回已捕获的请求快照。
    pub async fn recorded(&self) -> Vec<RecordedRequest> {
        self.requests.lock().await.clone()
    }
}

/// 启动 mock 服务器：依次为前 `responses.len()` 个连接各返回一条响应体（均 200）。
///
/// 每条响应都带 `Connection: close`，强制 reqwest 每个请求新建连接，
/// 从而让「第 N 个请求 ↔ 第 N 条响应」严格一一对应（避免连接池复用打乱顺序）。
pub async fn spawn(responses: Vec<String>) -> MockServer {
    spawn_with_statuses(responses.into_iter().map(|b| (200u16, b)).collect()).await
}

/// 启动 mock 服务器：按到达顺序返回预设的 `(状态码, 响应体)`。
///
/// 每条响应都带 `Connection: close`，强制 reqwest 每个请求新建连接，
/// 从而让「第 N 个请求 ↔ 第 N 条响应」严格一一对应（避免连接池复用打乱顺序）。
/// 状态非 200 用于验证适配器对 HTTP 错误（如 500）的自愈处理。
pub async fn spawn_with_statuses(responses: Vec<(u16, String)>) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定回环地址失败");
    let addr = listener.local_addr().expect("读取本地地址失败");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let sink = requests.clone();
    tokio::spawn(async move {
        for (status, body) in responses {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            sink.lock().await.push(read_request(&mut socket).await);
            let response = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                status_reason(status),
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
        }
    });
    MockServer {
        base_url: format!("http://{addr}"),
        requests,
    }
}

/// HTTP 状态码对应的原因短语：仅用于满足 HTTP 状态行格式，reqwest 只解析数字本体。
fn status_reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

/// 读取一个完整请求：先读至头部终止符 `\r\n\r\n`，再按 `Content-Length` 补齐正文。
async fn read_request(socket: &mut TcpStream) -> RecordedRequest {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if find_head_end(&buf).is_some() || buf.len() > 64 * 1024 {
            break;
        }
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head_end = find_head_end(&buf).unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let want = parse_content_length(&head);
    let mut body = buf[head_end..].to_vec();
    while body.len() < want {
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(want);
    RecordedRequest {
        head,
        body: String::from_utf8_lossy(&body).to_string(),
    }
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn parse_content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .and_then(|v| v.trim().parse().ok())
        })
        .unwrap_or(0)
}
