# ganyu-agent 文档总索引

> 按「入门 → 指南 → 架构 → 决策 → 安全」组织；全部文档以对标范式（Pi/OpenClaw/Hermes/Prime）为框架。

## 入门
- [根 README](../README.md) — 项目总览：特性 / 快速开始 / 架构一图 / 导航
- [install.md](install.md) — 安装：一条命令（默认免编译下载 release）/ cargo / 源码 + 特性矩阵 + 卸载 + FAQ

## 指南
- [config-guide.md](config-guide.md) — **配置模型指导**：`setup` 向导 + env 全量清单 + 4 个场景模板（离线/开发/生产/容器）+ 基线自检
- [usage.md](usage.md) — CLI 使用：setup/model/update/gateway/chat/run… 全部子命令 + 示例 + FAQ
- [development.md](development.md) — 开发：构建 / 测试 / 特性 / 扩展 / 约定 / **发布流程（CI+tag）** / 贡献

## 架构
- [architecture.md](architecture.md) — 架构：对标范式映射 / 分层 / 请求旅程 / 抽象 / 模块职责

## 决策记录（ADR，紧凑卡）
| 文档 | 决策 |
|------|------|
| [ADR-001](ADR-001-architecture.md) | 会话 UUID + 统一字符串值 + 三抽象层 |
| [ADR-002](ADR-002-multi-paradigm.md) | Unit×Workflow 多范式抽象 |
| [ADR-003](ADR-003-defect-vulnerability-audit.md) | 安全审计（5C/3H/6M/2L） |
| [ADR-004](ADR-004-complement-best-practices.md) | 2026 开源对标 |
| [ADR-005](ADR-005-remediation-plan.md) | P0–P3 修复（失败闭环） |
| [ADR-006](ADR-006-structure-engineering-cache.md) | 工程化/缓存/审计/配置 |
| [ADR-007](ADR-007-install-distribution.md) | 安装与分发（v2：CI 发布 + 免编译下载） |
| [ADR-008](ADR-008-out-of-box.md) | 开箱即用（配置文件/REPL/doctor/模型接入） |

## 安全
- [SECURITY.md](../SECURITY.md) — 12 层防线对照表 + 部署建议 + 漏洞报告

## 代码地图
```
core/      抽象与编排（llm/memory/agent/loop_/unit/workflow）
ext/       能力面（工具/插件/技能）
knowledge/ 知识面（MDL/SAG）
routing/   网关（级联+lkgp+缓存+审计）
heal/      自愈（重试/熔断/级联/限速）
security.rs / sandbox.rs   安全执行面 + 进程沙箱
config.rs / cache.rs / observe.rs   工程面
```
