//! 多范式编排层：把统一的 `Unit` 按不同协调策略组合成完整 agent 系统。
//!
//! 每个范式都是一个 `Workflow`：接收共享 `RunContext` 与输入，返回输出。
//! 区别只在"如何编排多个 Unit"：
//! - `single`      ：单 agent（Unit 内部即 ReAct 循环）—— 覆盖"单 agent"与"ReAct"。
//! - `plan_execute`：先规划（拆子任务）再逐步执行 —— Plan & Execute。
//! - `multi_agent` ：多个 agent 按轮次传递上下文 —— 多 agent 协作。
//! - `router`      ：分类器把请求派发给最匹配的 agent/skill —— Router。
//! - `blackboard`  ：agent 向共享黑板写贡献，合成器读整块黑板 —— Blackboard。
//! - `graph`       ：DAG 拓扑执行，节点间按边传递数据 —— Graph Workflow。
//!
//! 全部离线可跑：默认 `LocalBackend` / `LocalReasoner` 兜底；接真模型时同一套接口自动升级。

pub mod blackboard;
pub mod graph;
pub mod multi_agent;
pub mod plan_execute;
pub mod router;
pub mod single;

pub use blackboard::BlackboardWorkflow;
pub use graph::{GraphBuilder, GraphWorkflow};
pub use multi_agent::MultiAgentWorkflow;
pub use plan_execute::{LocalPlanner, PlanExecuteWorkflow};
pub use router::{KeywordRouter, Router, RouterWorkflow};
pub use single::SingleWorkflow;

use crate::core::unit::{RunContext, Unit};
use crate::error::GanyuResult;
use crate::value::Value;
use async_trait::async_trait;

/// 范式编排抽象：所有协调策略的统一入口。
#[async_trait]
pub trait Workflow: Send + Sync {
    /// 范式名（用于 CLI 选择 / 可观测）。
    fn mode(&self) -> &str;
    /// 跑一次完整编排，返回最终输出（统一 `Value`）。
    async fn run(&self, ctx: &RunContext, input: &Value) -> GanyuResult<Value>;
}

/// 把一个 `Unit` 包成 `dyn Workflow`（单 agent / ReAct 场景）。
pub fn as_workflow(unit: std::sync::Arc<dyn Unit>) -> SingleWorkflow {
    SingleWorkflow::new(unit)
}
