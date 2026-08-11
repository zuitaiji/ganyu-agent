# ADR-001: ganyu-agent 总体架构与关键决策

## Status
Accepted

## Context
基于备份目录的规划文档（对话/执行面 + 知识/分析面，OpenViking 记忆脊柱 + WrenAI MDL + SAG 管道 + 模型路由网关），
要落地一个**有温度、能自进化、可拓展、可自愈**的完备 agent 系统。原始设计文档是骨架（`sag_pipeline.py` 全是 TODO，
依赖 OpenViking / WrenAI / OmniRoute / 真 LLM 等重服务）。用户硬约束：Rust、会话 UUID、统一数据类型为 String、抽象层、全量发挥 Rust 特性。

## Decision
1. **语言与零依赖默认**：Rust。默认特性零网络依赖即可编译运行（本地兜底保证"一定会说话"）；
   `--features network` 才引入 reqwest/TLS，接真实 LLM。这是把"自愈"做成一等公民：外部后端不可用时自动降级本地。
2. **统一数据类型 `Value(String)`**：所有载荷（消息、记忆、工具 I/O、SQL）收敛为字符串 newtype，
   实现 `From<&str/i64/f64/bool>`。代价：数值需自行解析（如 `calc`）；收益：跨边界（HTTP/JSON/进程间）零歧义、可序列化、可审计。
3. **抽象层三 trait + 编排**：
   - `LlmBackend`（模型后端）、`Memory`（记忆）、`Tool`（工具）三个 trait 定义能力边界；
   - `Gateway` 做路由（级联 + lkgp 粘成功路径 + 熔断器）；`Agent` 做编排；`heal` 提供重试/熔断/级联。
   - 外部重服务走适配器 + 本地降级，默认不编译进二进制。
4. **会话 UUID 贯通**：`SessionId(Uuid)` 贯穿 `Agent` / `SAG` / 记忆提交。本次深化修复了 `Memory::commit` 原先用
   `SessionId::new()` 造随机会话的缺陷，改为显式传入真实会话；并新增 `load_session` 实现跨重启续接（自进化）。
5. **自愈错误分流**：`GanyuError` 区分 `BackendUnavailable`（可重试：网络抖动、5xx、408、429）与
   `BackendError`（致命：4xx 鉴权/请求错误）。网关熔断 + Agent 重试只对"可重试"类生效，避免对致命错误盲目重试放大故障。
6. **全量 agent 工作流（ReAct 推理循环）**：`core/loop` 把单条消息升级为多步「思考→行动→观察」循环，
   对齐开源 agent 主流程。决策由可插拔 `Reasoner` 驱动：`LocalReasoner` 离线解析 `@tool arg` 脚本 +
   关键字路由到技能；接真模型时由 `LlmReasoner` 取代即自动进入多步深思（循环机制不变，仅决策源升级）。
   每步写入可观测 `Step` 轨迹（Thought/Action/Observation/Final），落到会话记忆，供调试与续接。
7. **可生长的特性技能（Skill）**：区别于原子 `Tool`，`Skill` 是含 `Call`/`Note`/`Summarize` 步骤的复合程序，
   包装为 `skill:<name>` 工具注册进 `ToolRegistry`，从而既能 `@skill:...` 直接调用，也能被 `Reasoner` 自动路由命中。
   内置 `summarize` / `troubleshoot` / `kb_query` 三个特性技能；业务专属技能只需 `SkillBook::register_skill`，
   无需改核心代码。这是"生长出特色技能"的机制落点。

## Consequences
- 易：离线编译运行、测试可全绿；能力可热插拔（工具/插件/技能免重编译）。
- 难/代价：String 统一带来解析成本；降级路径可能掩盖真实后端故障（已用熔断 + 错误分类缓解）；
  真实 LLM 调用需用户自备端点与密钥，本仓库不内置任何凭证。
- `@tool` 脚本语法仅把"同行参数"传给工具，多行内容（如 `file_write` 的正文）需走 `tools.call` 或
  `skill` 子命令/文件已存在场景；自然语言路由到文件类技能时由 `file_read` 路径抽取容错兜底。
- 可进化：所有后端均为 trait，新增 OpenViking / OmniRoute / 本地模型只需实现 trait 并 `register`；
  新增能力只需注册 Tool / Skill，核心循环与编排不变。

## 备选方案（被否决）
- 直接用 Python 落地原始骨架：违背用户 Rust 硬约束，且重服务依赖无法离线验证。
- 默认编译进真 LLM 后端：增大二进制与 C/TLS 依赖，违背"零依赖可跑"。
- `commit` 不绑会话（原实现）：会话轨迹与真实会话脱钩，自进化名不副实 —— 已修复。
- 把技能做成原子 Tool：失去"多步复合"语义，无法表达"读文件→摘要"这类程序，也不利于自动路由。
