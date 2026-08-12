# ADR-002: 多范式抽象（Unit × RunContext × Workflow）

## Status
Accepted

## Context
对齐开源 agent 的七种编排范式（single/react/plan/multi/router/blackboard/graph），
若逐范式重复接线会造成大量脚手架。

## Decision
- 抽离原子 `Unit`（`name` + `run(ctx,input)`）；`RunContext` 共享会话/黑板/记忆/网关/工具/技能。
- `Workflow` trait 统一 `run(ctx,input)`，范式 = 对 `Unit` 的**协调策略**；
  ReAct 是 `Unit`(Agent) 的**内部行为**。
- 7 实现：`SingleWorkflow` / `PlanExecuteWorkflow`(LocalPlanner) / `MultiAgentWorkflow` /
  `RouterWorkflow`(KeywordRouter) / `BlackboardWorkflow` / `GraphBuilder`（构造即校验环）。
- CLI：`agent "任务" --mode <范式>`；离线可跑（Local 兜底）。

## Consequences
- 易：新范式只实现 `Workflow`；所有范式复用自愈/记忆/工具接线。
- 难：抽象层需稳定（以 trait 文档 + 集成测试约束）。

## 验证
8 个工作流集成测试全绿（含 Graph 环检测、多 agent 转录、黑板共享）。
