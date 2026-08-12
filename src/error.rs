//! 统一错误类型。`thiserror` 派生，全链路用 `Result<T, GanyuError>` + `?`。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GanyuError {
    #[error("all LLM backends failed: {0}")]
    AllBackendsFailed(String),

    #[error("tool '{0}' not found")]
    ToolNotFound(String),

    #[error("tool '{0}' failed: {1}")]
    ToolFailed(String, String),

    #[error("validation failed: {0:?}")]
    ValidationFailed(Vec<String>),

    #[error("backend '{0}' unavailable (retryable)")]
    BackendUnavailable(String),

    #[error("backend '{0}' fatal (auth/bad request, not retryable)")]
    BackendError(String),

    #[error("plugin error: {0}")]
    Plugin(String),

    /// 安全策略拒绝（失败闭环：默认拒绝，需显式开启才放行）。
    #[error("forbidden by security policy: {0}")]
    Forbidden(String),

    /// 检测到疑似注入（H2 SQL 注入防护）。
    #[error("possible injection detected: {0:?}")]
    Injection(Vec<String>),

    /// 触发速率限制（M2）。
    #[error("rate limited: {0}")]
    RateLimited(String),

    /// SSRF 防护拦截（C5）。
    #[error("ssrf guard blocked request: {0}")]
    Ssrf(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("regex: {0}")]
    Regex(#[from] regex::Error),

    #[error("http: {0}")]
    Http(String),

    #[error("workflow: {0}")]
    Workflow(String),
}

pub type GanyuResult<T> = Result<T, GanyuError>;
