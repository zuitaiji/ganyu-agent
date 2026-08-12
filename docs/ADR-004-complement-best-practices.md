# ADR-004: 2026 开源 Agent 安全边界对标

## Status
Accepted

## Context
为补齐防护短板，调研 2026 主流开源 agent 的**能力边界与实际防护**，取长补短。

## Decision（对标结论）
| 框架 | 关键边界 | 借鉴 |
|------|----------|------|
| smolagents 1.26 | CVE-2025-5120 沙箱逃逸（9.9） | 沙箱不等于安全 |
| LangGraph 1.10+ | checkpointer RCE 链（CVE-2025-67644 等） | 状态持久化要当攻击面 |
| CrewAI | 多重 fail-open 降级（CVE-2026-2275 等） | 默认拒绝而非放行 |
| AutoGen/AG2 | Docker+gVisor 沙箱 | 容器隔离 |
| OpenHands v0.21 | 每会话 Docker 默认隔离 | 隔离是默认项 |
| Agno v2.7 | Firecracker 微 VM | 轻量 VM 隔离 |
| Google ADK | CVE-2026-18236 工具确认伪造（9.3） | 工具调用须留痕 |
| Pydantic AI | 类型化工具 IO | 类型安全 |
| Letta | 记忆即攻击面 | 记忆加密（H1） |

## Consequences
- 提炼三条基线：**默认拒绝 / 显式开启 / 可证明隔离**，写入 ADR-005/006。
- ganyu 采取「本地轻量沙箱 + 容器兜底」组合（诚实边界，见 ADR-006/SECURITY）。

## 验证
逐框架核对版本 + CVE + 来源 URL 收录于本 ADR 原稿（已随重构精简）。
