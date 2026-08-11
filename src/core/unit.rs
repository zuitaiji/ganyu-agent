//! 统一运行时抽象：`Unit` + `RunContext`。
//!
//! 这是"适配全部 agent 框架"的基石。所有范式都建立在同一个原子单元之上：
//! - **单 agent / ReAct**：`Unit` 的内部就是一次 ReAct 推理循环（`Agent::run`）。
//! - **Plan&Execute / 多 agent / Router / Blackboard / Graph**：都是对多个 `Unit`
//!   的不同*协调策略*（见 `workflow` 模块），而非另起一套类型。
//!
//! `RunContext` 在所有范式间共享同一份上下文：会话 UUID、黑板（Blackboard 范式）、
//! 记忆、网关、工具表、技能书。这样 Unit 既能在单 agent 下独立跑，也能在复杂编排里
//! 共享状态——而接口不变。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::core::memory::DynMemory;
use crate::error::GanyuResult;
use crate::ext::{SkillBook, ToolRegistry};
use crate::routing::Gateway;
use crate::session::SessionId;
use crate::value::Value;
use async_trait::async_trait;

/// 跨范式共享的运行时上下文。
///
/// 所有 `Unit` 都拿到同一个 `RunContext` 副本（Arc 内部可变状态），因此：
/// - 会话 UUID 自然贯通（与记忆提交一致）。
/// - Blackboard 范式用 `board` 做共享写空间；其他范式忽略它也不受影响。
/// - 记忆/网关/工具/技能全局唯一，避免重复构造。
pub struct RunContext {
    pub session: SessionId,
    board: Arc<Mutex<HashMap<String, Value>>>,
    pub memory: DynMemory,
    pub gateway: Arc<Gateway>,
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillBook>,
}

impl RunContext {
    pub fn new(
        session: SessionId,
        memory: DynMemory,
        gateway: Arc<Gateway>,
        tools: Arc<ToolRegistry>,
        skills: Arc<SkillBook>,
    ) -> Self {
        RunContext {
            session,
            board: Arc::new(Mutex::new(HashMap::new())),
            memory,
            gateway,
            tools,
            skills,
        }
    }

    /// 黑板读（Blackboard 范式；其他范式可忽略）。
    pub fn board_get(&self, key: &str) -> Option<Value> {
        self.board.lock().unwrap().get(key).cloned()
    }

    /// 黑板写（每个 `Unit` 跑完会把自身结果落盘到 key=role，实现共享）。
    pub fn board_set(&self, key: &str, value: Value) {
        self.board.lock().unwrap().insert(key.to_string(), value);
    }

    /// 黑板全量快照（合成器读取整块黑板）。
    pub fn board_all(&self) -> HashMap<String, Value> {
        self.board.lock().unwrap().clone()
    }
}

/// 原子运行时单元：所有 agent 范式的最小可执行单位。
///
/// `name` 同时用作黑板写入的 key（角色名）。`run` 接收共享上下文与输入，返回统一 `Value`。
#[async_trait]
pub trait Unit: Send + Sync {
    fn name(&self) -> &str;
    async fn run(&self, ctx: &RunContext, input: &Value) -> GanyuResult<Value>;
}
