# ganyu-agent docs 索引

> 工程化目录管理：所有设计/治理文档集中于此，按主题检索。

## 架构决策记录（ADR）
| 文档 | 主题 |
|------|------|
| [ADR-001](ADR-001-architecture.md) | 统一架构：会话 UUID + 统一字符串值 + 抽象层（Memory/LlmBackend/Tool） |
| [ADR-002](ADR-002-multi-paradigm.md) | 多范式编排：`Unit`/`RunContext`/`Workflow` 三层抽象（single/react/plan/multi/router/blackboard/graph） |
| [ADR-003](ADR-003-defect-vulnerability-audit.md) | 缺陷与漏洞全量审计（5C/3H/6M/2L + PoC） |
| [ADR-004](ADR-004-complement-best-practices.md) | 开源 agent 对标（smolagents/LangGraph/CrewAI/AutoGen/OpenHands/Agno/ADK/Pydantic AI/Letta） |
| [ADR-005](ADR-005-remediation-plan.md) | P0–P3 修复落地（失败闭环：exec 默认关/文件沙箱/SSRF/加密/注入防护/限速/审计） |
| [ADR-006](ADR-006-structure-engineering-cache.md) | 结构化/工程化/安全治理/缓存优化（对标 Pi·OpenClaw·Hermes·Prime） |
| [ADR-007](ADR-007-install-distribution.md) | 安装与分发（一键脚本 / cargo install / 特性矩阵 / 供应链安全） |

## 安装
- [install.md](install.md)：一键脚本（curl|sh / irm|iex）、cargo install、源码构建、特性矩阵、卸载、FAQ
- 脚本：[install.sh](../install.sh)（Linux/macOS/Git-Bash）、[install.ps1](../install.ps1)（Windows）

## 安全
- [SECURITY.md](../SECURITY.md)：安全治理基线（分层防线 / env 清单 / 生产加固 / 部署建议 / 漏洞报告）

## 代码地图（src/ 组织）
- `core/`：抽象层与编排（memory / llm / agent / loop_ / unit / workflow）
- `ext/`：能力面（工具注册 / 插件发现 / 技能）
- `knowledge/`：知识面（MDL 语义校验 / SAG 管道）
- `heal/`：自愈（重试 / 熔断 / 级联 / 限速）
- `routing/`：网关（级联 + lkgp + 缓存 + 审计）
- `security/`：安全执行面（文件沙箱 / SSRF / shell 开关 / 输出净化）
- `sandbox/`：进程级 FS 沙箱（Landlock，Linux-only）
- `config/`：工程化配置面（env 集中 + 基线自检）
- `cache/`：缓存层（LRU+TTL）
- `observe/`：审计日志（JSON Lines）
