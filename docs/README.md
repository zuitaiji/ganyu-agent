# ganyu-agent 文档总索引

> 工程化目录管理：全部文档按「入门 → 指南 → 架构 → 决策 → 安全」组织。

## 🚀 入门
| 文档 | 内容 |
|------|------|
| [根 README](../README.md) | 项目总说明：特性 / 快速开始 / 架构一图 / 目录 / 安全 |
| [install.md](install.md) | 安装指南：一键脚本（curl|sh / irm|iex）/ cargo / 源码 + 特性矩阵 + 卸载 + FAQ |

## 📖 指南
| 文档 | 内容 |
|------|------|
| [usage.md](usage.md) | CLI 使用指南：全部子命令 + 示例（推理 / 多范式 / SAG / 技能 / 会话续接 / 特性开关） |
| [development.md](development.md) | 开发指南：构建 / 测试 / 特性矩阵 / 扩展方法 / 工程约定 / 贡献流程 |

## 🏛 架构
| 文档 | 内容 |
|------|------|
| [architecture.md](architecture.md) | 架构概览：分层 / 请求旅程 / 核心抽象 / 模块职责表 / 安全边界 |

## 📐 架构决策记录（ADR）
| 文档 | 主题 |
|------|------|
| [ADR-001](ADR-001-architecture.md) | 统一架构：会话 UUID + 统一字符串值 + 抽象层 |
| [ADR-002](ADR-002-multi-paradigm.md) | 多范式编排：Unit / RunContext / Workflow 三层抽象 |
| [ADR-003](ADR-003-defect-vulnerability-audit.md) | 缺陷与漏洞全量审计（5C/3H/6M/2L + PoC） |
| [ADR-004](ADR-004-complement-best-practices.md) | 开源 agent 对标（smolagents/LangGraph/CrewAI/AutoGen/OpenHands/Agno/ADK 等） |
| [ADR-005](ADR-005-remediation-plan.md) | P0–P3 修复落地（失败闭环） |
| [ADR-006](ADR-006-structure-engineering-cache.md) | 结构化/工程化/安全治理/缓存优化（对标 Pi·OpenClaw·Hermes·Prime） |
| [ADR-007](ADR-007-install-distribution.md) | 安装与分发（脚本 / cargo / 供应链安全） |

## 🔒 安全
- [SECURITY.md](../SECURITY.md)：安全治理基线（12 层防线 / env 清单 / 生产加固 / 部署建议 / 漏洞报告）

## 代码地图（src/ 组织）
- 抽象与编排：`core/`（memory / llm / agent / loop_ / unit / workflow）
- 能力面：`ext/`（工具注册 / 插件发现 / 技能）
- 知识面：`knowledge/`（MDL 校验 / SAG 管道）
- 模型面：`routing/`（网关：级联 + lkgp + 缓存 + 审计）
- 自愈：`heal/`（重试 / 熔断 / 级联 / 限速）
- 安全执行面：`security.rs`（文件沙箱 / SSRF / shell 开关 / 净化）
- 进程沙箱：`sandbox.rs`（Landlock，Linux-only）
- 工程面：`config.rs`（配置+基线自检）/ `cache.rs`（LRU+TTL）/ `observe.rs`（审计日志）
