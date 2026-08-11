//! 对话/执行面编排：`Agent`。
//!
//! 把人格层（persona）、路由层（gateway）、记忆层（memory）、工具层（tools）、进化层（skills）
//! 串成一条消息的生命周期；任何失败都走 `heal` 自愈，最终降级到本地兜底而非崩溃。

use std::sync::{Arc, Mutex};

use crate::core::llm::Message;
use crate::core::memory::DynMemory;
use crate::error::GanyuResult;
use crate::ext::{SkillBook, ToolRegistry};
use crate::heal::with_retry_async;
use crate::persona::build_system_prompt;
use crate::session::SessionId;
use crate::value::Value;

pub struct Agent {
    pub gateway: Arc<crate::routing::Gateway>,
    pub memory: DynMemory,
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillBook>,
    pub persona: Value,
    pub session: SessionId,
    history: Mutex<Vec<Message>>,
}

impl Agent {
    pub fn new(
        gateway: Arc<crate::routing::Gateway>,
        memory: DynMemory,
        tools: Arc<ToolRegistry>,
        skills: Arc<SkillBook>,
        session: SessionId,
    ) -> Self {
        let persona = build_system_prompt("");
        Agent {
            gateway,
            memory,
            tools,
            skills,
            persona,
            session,
            history: Mutex::new(Vec::new()),
        }
    }

    pub fn session(&self) -> SessionId {
        self.session
    }

    fn build_messages(&self, user: &Value) -> Vec<Message> {
        let mut v = vec![Message::system(self.persona.clone())];
        v.extend(self.history.lock().unwrap().iter().cloned());
        v.push(Message::user(user.clone()));
        v
    }

    /// 处理一条用户消息：工具分发 → 模型补全（自愈）→ 降级 → 记忆提交。
    pub async fn respond(&self, user_msg: &Value) -> GanyuResult<Value> {
        // 工具分发：@name arg
        if let Some((name, arg)) = parse_tool_call(user_msg.as_str()) {
            return self.tools.call(&name, &Value(arg)).await;
        }

        let messages = self.build_messages(user_msg);
        // 自愈：网关做级联+熔断；这里再包一层重试兜底。
        let reply = with_retry_async(|| self.gateway.complete(&messages), 2, std::time::Duration::from_millis(20))
            .await
            .unwrap_or_else(|_| self.degrade(user_msg));

        self.history.lock().unwrap().push(Message::user(user_msg.clone()));
        self.history.lock().unwrap().push(Message::assistant(reply.clone()));

        // 记忆层自愈：会话轨迹写本地（失败不致命）
        let trace = Value(
            serde_json::json!({
                "session": self.session.as_string(),
                "user": user_msg.as_str(),
            })
            .to_string(),
        );
        let _ = self.memory.commit(&self.session, &trace).await;

        Ok(reply)
    }

    /// 续接会话：若记忆中存在该会话的轨迹，注入为开场上下文（跨重启自进化）。
    /// 返回是否有可续接的历史。
    pub async fn resume(&self) -> bool {
        match self.memory.load_session(&self.session).await {
            Ok(Some(trace)) => {
                self.history.lock().unwrap().push(Message::system(Value(
                    format!("[续接会话 {}] 上次轨迹：{}", self.session, trace),
                )));
                true
            }
            _ => false,
        }
    }

    fn degrade(&self, user: &Value) -> Value {
        let preview: String = user.as_str().chars().take(40).collect();
        Value(format!("[降级响应] 模型层暂时不可用，已本地兜底。你的消息：{preview}"))
    }
}

/// 简单工具调用语法：`@toolname arg...`。
fn parse_tool_call(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    let rest = s.strip_prefix('@')?;
    let mut it = rest.splitn(2, char::is_whitespace);
    let name = it.next()?.to_string();
    let arg = it.next().unwrap_or("").to_string();
    Some((name, arg))
}
