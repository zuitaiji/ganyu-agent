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
