//! 多 agent 范式：多个 agent 按轮次协作，每轮把"当前进展"透传给下一个 agent。
//!
//! 每个 agent 是独立 `Unit`（不同角色），在自己的 ReAct 循环里消化进展并产出新进展；
//! 轮次结束时输出整段协作轨迹的尾部作为最终答案。离线可用不同角色 agent 演示分工。

use std::sync::Arc;

use async_trait::async_trait;

use crate::core::unit::{RunContext, Unit};
use crate::error::GanyuResult;
use crate::value::Value;

use super::Workflow;

pub struct MultiAgentWorkflow {
    units: Vec<Arc<dyn Unit>>,
    max_rounds: usize,
}

impl MultiAgentWorkflow {
    pub fn new(units: Vec<Arc<dyn Unit>>, max_rounds: usize) -> Self {
        MultiAgentWorkflow {
            units,
            max_rounds: max_rounds.max(1),
        }
    }
}

#[async_trait]
impl Workflow for MultiAgentWorkflow {
    fn mode(&self) -> &str {
        "multi_agent"
    }

    async fn run(&self, ctx: &RunContext, input: &Value) -> GanyuResult<Value> {
        if self.units.is_empty() {
            return Ok(input.clone());
        }
        let mut transcript = format!("初始任务：{}\n", input);
        for round in 0..self.max_rounds {
            for unit in &self.units {
                let prompt = format!(
                    "[第 {} 轮 | 角色 {}]\n当前进展：\n{}\n\n请基于以上进展推进你的部分，只输出你的新增贡献。",
                    round + 1,
                    unit.name(),
                    transcript
                );
                let out = unit.run(ctx, &Value(prompt)).await?;
                transcript.push_str(&format!("\n[{}] {}\n", unit.name(), out));
            }
        }
        // 最终答案取轨迹尾部（最后一个 agent 的贡献）。
        Ok(Value(transcript))
    }
}
