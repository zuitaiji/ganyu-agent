# ADR-008: 开箱即用（配置模型 → 直接对话）

## Status
Accepted

## Context
真实使用反馈：默认构建即使配了 `OPENAI_API_*` 仍是"本地兜底"，因为 `LlmReasoner` 从未实现
（`LocalReasoner` 不调用 LLM 后端）；且用户期望 OpenClaw/Hermes 式体验——**启动即对话**，而非手动 export。

## Decision
- **D1 LlmReasoner 落地**：`Reasoner::decide` 改 async（async_trait）；`LlmReasoner` 把已知工具清单
  注入系统提示，模型输出 `@tool 参数` 则行动、否则直接作答；配置 `OPENAI_API_BASE/KEY` 自动启用。
- **D2 配置文件一站式**：`~/.ganyu/config.toml`（`[model] base_url/api_key/model`）启动自动加载；
  优先级 `$GANYU_CONFIG` > `~/.ganyu/config.toml` > `./ganyu.toml`；**env 优先于文件**。
- **D3 交互式 REPL**：`chat` 在终端（`is_terminal`）下进入多轮对话循环——同一会话续接上下文，
  `/quit`/Ctrl+C 退出；管道输入保持单次兼容。
- **D4 doctor 环境诊断**：`ganyu-agent doctor` 输出编译特性/配置文件/模型配置/网关后端/能力面/
  记忆状态与建议（对标 OpenClaw 开箱自检）。
- **D5 兼容性修复（接真模型的四个缺口）**：reqwest `system-proxy`（环境代理）；
  `Role` `serde(rename_all="lowercase")`（OpenAI 兼容网关 400 根因）；网关 `ordered_names`
  本地兜底永远排最后（否则真后端被饿死）；`OPENAI_MODEL` 模型名可配置 + `reasoning_content` 推理模型回退。

## Consequences
- 易：写一次配置 → `ganyu-agent chat` 即真实模型对话；`doctor` 秒级定位配置问题；
  安装脚本引导开箱配置。
- 难：配置文件含 API key（提示不提交仓库；env 优先便于 CI 注入）；REPL 语义需文档明确
  （同会话上下文延续是特性而非缺陷）。

## 验证
`ganyu-agent run "你好"`（无任何 env，靠 config.toml）→ agnes-2.5-flash 真实回复 ✅；
`chat` 管道单次回复 ✅；`doctor` 输出 ✅；selftest 9/9；release 构建零警告。
提交：`cfaac68`（feat）+ `a2c9a70`（docs）。
