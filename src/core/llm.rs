//! 抽象层之一：模型后端 `LlmBackend`。
//!
//! - `LocalBackend`：零密钥、确定性的本地兜底，保证任何环境下 agent 都能"说话"。
//! - `OpenAiBackend`：OpenAI 兼容 `/v1` 端点（可指向 OmniRoute / Ollama / 任意网关），
//!   仅在 `--features network` 下编译，避免默认构建引入 TLS/C 依赖。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::GanyuResult;
use crate::value::Value;

/// 对话角色（Rust `enum` 表达类型安全，而非裸字符串）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
}

/// 一条消息：角色 + 统一字符串内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Value,
}

impl Message {
    pub fn system(content: impl Into<Value>) -> Self {
        Message { role: Role::System, content: content.into() }
    }
    pub fn user(content: impl Into<Value>) -> Self {
        Message { role: Role::User, content: content.into() }
    }
    pub fn assistant(content: impl Into<Value>) -> Self {
        Message { role: Role::Assistant, content: content.into() }
    }
}

/// 模型后端抽象。所有后端共享 `Arc<dyn LlmBackend + Send + Sync>`。
#[async_trait]
pub trait LlmBackend: Send + Sync {
    fn name(&self) -> &str;
    fn is_local(&self) -> bool {
        false
    }
    async fn complete(&self, messages: &[Message]) -> GanyuResult<Value>;
}

/// 便于在 Gateway / Agent 中统一持有。类型对象自带 `Send + Sync`（trait 已声明超约束）。
pub type DynBackend = std::sync::Arc<dyn LlmBackend + Send + Sync>;

/// 本地兜底后端：无网络、无密钥。
pub struct LocalBackend;

#[async_trait]
impl LlmBackend for LocalBackend {
    fn name(&self) -> &str {
        "local"
    }
    fn is_local(&self) -> bool {
        true
    }
    async fn complete(&self, messages: &[Message]) -> GanyuResult<Value> {
        let user = messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let preview: String = user.chars().take(60).collect();
        Ok(Value(format!(
            "[本地兜底] 收到：{preview}（未配置联网模型端点；设置 OPENAI_API_BASE/OPENAI_API_KEY 并以 --features network 编译即可升级）"
        )))
    }
}

/// 真实联网后端（可选编译）。
#[cfg(feature = "network")]
pub struct OpenAiBackend {
    base_url: String,
    api_key: String,
    model: String,
    name: String,
}

#[cfg(feature = "network")]
impl OpenAiBackend {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        OpenAiBackend {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            name: format!("openai:{model}"),
        }
    }
}

#[cfg(feature = "network")]
#[async_trait]
impl LlmBackend for OpenAiBackend {
    fn name(&self) -> &str {
        &self.name
    }
    async fn complete(&self, messages: &[Message]) -> GanyuResult<Value> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0,
        });
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| crate::error::GanyuError::Http(e.to_string()))?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| GanyuError::Http(e.to_string()))?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(Value(content))
    }
}
