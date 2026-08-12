# ADR-001: 统一架构（会话 UUID · 统一字符串值 · 抽象层）

## Status
Accepted

## Context
需要一套「既有温度、又可工程化」的 agent 底座：跨会话一致性、载荷收敛、可替换后端。

## Decision
- **会话 UUID**：`SessionId(Uuid)` 贯穿 Agent / SAG / 记忆提交，支持跨重启续接（自进化锚点）。
- **统一字符串值**：`Value(String)` newtype 收敛全部载荷（消息/记忆/工具 IO/SQL），`From` 全类型提升。
- **三层抽象**：`Memory` / `LlmBackend` / `Tool` + `Gateway`(路由) + `Agent`(编排) + `heal`(自愈)。
- **离线优先**：`LocalBackend`/`LocalMemory` 兜底，默认构建零网络依赖。

## Consequences
- 易：任意后端可替换、载荷无类型漂移、会话可续接。
- 难：字符串载荷牺牲强类型（以 newtype + 文档约束弥补）。

## 验证
28+ 测试全绿；`--session` 续接闭环；默认构建无外部依赖。
