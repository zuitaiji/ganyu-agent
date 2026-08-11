//! 单 agent / ReAct 范式：把单一 `Unit`（内部即 ReAct 循环）直接作为完整工作流运行。
//!
//! 这是最基础的范式，也是其它所有范式复用的原子。当 `unit` 是 `Agent` 时，跑的就是
//! 完整的「感知—推理—行动—观察」多步循环（默认离线兜底，接真模型后自动多步深思）。

use std::sync::Arc;

use async_trait::async_trait;

use crate::core::unit::{RunContext, Unit};
use crate::error::GanyuResult;
use crate::value::Value;

use super::Workflow;

pub struct SingleWorkflow {
    unit: Arc<dyn Unit>,
}

impl SingleWorkflow {
    pub fn new(unit: Arc<dyn Unit>) -> Self {
        SingleWorkflow { unit }
    }
}

#[async_trait]
impl Workflow for SingleWorkflow {
    fn mode(&self) -> &str {
        "single"
    }

    async fn run(&self, ctx: &RunContext, input: &Value) -> GanyuResult<Value> {
        self.unit.run(ctx, input).await
    }
}
