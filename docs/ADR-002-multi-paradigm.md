# ADR-002：统一 Unit 抽象与多范式 agent 框架

- 状态：Accepted（2026-08-12）
- 决策者：SoftwareArchitect（ganyu-agent）
- 关联：ADR-001（Rust / 零依赖 / Value(String) / 三 trait 抽象 / 会话 UUID / 错误分流）

## Context

用户要求 ganyu-agent 适配业界主流 agent 框架的全部场景：单 agent、ReAct、Plan & Execute、
多 agent、Router（router-skill）、Blackboard、Graph Workflow。

原实现已有 `Agent`（单 agent + ReAct 循环）作为原子编排，但缺乏"多 agent / 路由 / 黑板 / 图"
等协调模式。若为每个范式各写一套类型，会重复大量脚手架、破坏一致性、且难以共享自愈/记忆/工具。

## Decision

引入**统一运行时抽象**，让所有范式都建立在同一个原子单元之上：

- `Unit` trait：`name(&self) -> &str` + `async fn run(&self, ctx: &RunContext, input: &Value) -> GanyuResult<Value>`。
  这是唯一原子。任何 agent 能力（含 `Agent` 本身）都实现 `Unit`。
- `RunContext`：跨范式共享上下文——`session`(UUID) / `board`(黑板) / `memory` / `gateway` / `tools` / `skills`。
  所有 `Unit` 拿到同一份副本（Arc 内部可变），天然共享状态而不破坏接口。
- `Agent` 实现 `Unit`：内部仍是 ReAct 循环；跑完把结果写入 `ctx.board`（key=角色），
  使其在 Blackboard/Graph 编排里自动贡献到共享空间。新增 `role` 字段 + `with_role` 以支持职责区分。

范式 = 对 `Unit` 的不同**协调策略**（`Workflow` trait，统一 `run(ctx, input)`）：

| 范式 | 实现 | 协调语义 |
|------|------|----------|
| 单 agent | `SingleWorkflow` | 直接跑一个 `Unit` |
| ReAct | `SingleWorkflow`（unit=Agent） | Agent 内部多步循环 |
| Plan & Execute | `PlanExecuteWorkflow` | 规划器拆子任务 → 逐步执行 → 合成 |
| 多 agent | `MultiAgentWorkflow` | 多 `Unit` 按轮次传递上下文 |
| Router | `RouterWorkflow` + `Router` trait | 分类器派发到专精 `Unit`，否则 fallback |
| Blackboard | `BlackboardWorkflow` | 各 `Unit` 写共享黑板，合成器读整块黑板 |
| Graph | `GraphWorkflow`（`GraphBuilder`） | DAG 拓扑序执行，边传数据，构造即校验环/孤立节点 |

- **离线优先**：默认 `LocalPlanner`（按连接词朴素拆分）、`KeywordRouter`（关键字路由）、
  `LocalBackend` / `LocalReasoner` 兜底。接真模型时只需替换 planner/router/reasoner，
  同一套编排接口自动升级为多步深思。
- **可拓展**：新范式 = 实现 `Workflow`；新 agent = `Agent::with_role` 或自定义 `Unit`；
  新路由/规划 = 实现 `Router` / 换 `planner`。均与核心解耦。

## Consequences

- 易：七个范式共用 `Unit`/`RunContext`/`Workflow`，零重复脚手架；全部离线可测（28 个测试通过）。
- CLI：`agent "任务" --mode <single|react|plan|multi|router|blackboard|graph>` + `modes` 列举。
- 已知限制：
  - 离线无真 LLM 时，各范式输出为本地兜底文本（机制已验证，语义需接模型才完整）——符合"零依赖可跑"。
  - `Unit` 间数据传递以 `Value`(字符串) 为主；Graph 多前驱时按 `\n` 合并。
  - Blackboard/Multi 目前顺序执行（共享活黑板已生效）；需并行可后续用 `tokio::spawn` 扩展，
    接口不变。
- 可进化：所有协调器均为 trait/`Arc<dyn Unit>`，新增 OpenViking 记忆 / OmniRoute 路由 /
  LlmReasoner 即插即用，不改动范式代码。

## 备选方案（被否决）

- 每范式独立类型体系：重复自愈/记忆/工具接线，维护成本高，否决。
- 用宏生成各范式：可读性差、调试困难，且 Pattern 差异大，否决。
- 只做"单 agent + ReAct"：不满足"全部框架"的明确要求，否决。
