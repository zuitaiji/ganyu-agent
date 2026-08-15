//! Plan & Execute 范式：先"规划"把任务拆成有序子步骤，再逐步"执行"，最后"合成"。
//!
//! - `planner`：产出计划（`Unit`，离线用 `LocalPlanner` 朴素拆分；接真模型可产出真实计划）。
//! - `executor`：逐个子步骤执行（通常是带工具/技能的 `Agent`，自动 ReAct）。
//! - 合成：把所有子结果回灌给 executor 产出最终作答。
//!
//! 离线可跑：`LocalPlanner` 按连接词拆分请求；executor 经技能/工具真实产出结果。

use std::sync::Arc;

use async_trait::async_trait;

use crate::core::unit::{RunContext, Unit};
use crate::error::GanyuResult;
use crate::value::Value;

use super::Workflow;

/// 单轮计划可执行的最大步数，防止 LLM 产出失控的长计划拖垮进程（F-05）。
const MAX_PLAN_STEPS: usize = 20;

/// 离线规划器：按中文连接词把请求拆成有序子任务。接真模型时替换为 LLM 规划器即可。
pub struct LocalPlanner;

#[async_trait]
impl Unit for LocalPlanner {
    fn name(&self) -> &str {
        "planner"
    }

    async fn run(&self, _ctx: &RunContext, input: &Value) -> GanyuResult<Value> {
        Ok(Value(decompose(input.as_str())))
    }
}

/// 朴素分解：按连接词切分；单句则原样作为一步。
fn decompose(text: &str) -> String {
    let sep: &[&str] = &["然后", "并且", "以及", "；", ";", "，然后", " 并且 ", "再"];
    let mut parts: Vec<String> = vec![text.to_string()];
    for s in sep {
        let mut next = Vec::new();
        for p in &parts {
            for piece in p.split(s) {
                let t = piece.trim().to_string();
                if !t.is_empty() {
                    next.push(t);
                }
            }
        }
        parts = next;
    }
    parts.join("\n")
}

pub struct PlanExecuteWorkflow {
    planner: Arc<dyn Unit>,
    executor: Arc<dyn Unit>,
}

impl PlanExecuteWorkflow {
    pub fn new(planner: Arc<dyn Unit>, executor: Arc<dyn Unit>) -> Self {
        PlanExecuteWorkflow { planner, executor }
    }
}

#[async_trait]
impl Workflow for PlanExecuteWorkflow {
    fn mode(&self) -> &str {
        "plan_execute"
    }

    async fn run(&self, ctx: &RunContext, input: &Value) -> GanyuResult<Value> {
        // 1) 规划
        let plan_val = self.planner.run(ctx, input).await?;
        // 限制计划步数上限，防止 LLM 产出失控的长计划拖垮进程（F-05）。
        let steps: Vec<String> = plan_val
            .as_str()
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .take(MAX_PLAN_STEPS)
            .collect();

        // 2) 逐步执行
        let mut results: Vec<String> = Vec::new();
        for step in &steps {
            let out = self.executor.run(ctx, &Value(step.clone())).await?;
            results.push(format!("- 子任务「{step}」→ {out}"));
        }

        // 3) 合成最终作答
        let synth = format!(
            "原始任务：{}\n执行计划（{} 步）：\n{}\n已完成的各步结果见上。请给出整合后的最终结论。",
            input,
            steps.len(),
            results.join("\n")
        );
        self.executor.run(ctx, &Value(synth)).await
    }
}
