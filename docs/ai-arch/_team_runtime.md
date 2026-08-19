# AICoding 架构专家团 — 运行时决策与编排清单

> 项目：ganyu-agent（Rust CLI AI agent）
> 仓库：`d:\workbuddy_all\harness_all\ganyu-agent`
> 主理人：齐构成（aicoding-architecture-expert-team-lead）
> 团队：`aicoding-ganyu-arch`
> 启动时间：2026-08-19

## 1. 运行时决策对象（G0 已确认）
| 字段 | 值 | 说明 |
|------|----|------|
| need_ingest | true | 启用 `knowledge-ingest-engineer`，将工程审计报告+现有 docs+代码 归一化为 `material_digest.md` |
| need_research | true | 启用 `research-analyst`，基于 material_digest 做行业标杆与方案加权评分 |
| need_cloud_baseline_check | true | 平台架构师在部署设计阶段按需用 `CloudQ` 做云/自托管资源现状核对 |

## 2. 架构范围（G0 已确认）
- **整体方案**：为整个 ganyu-agent 产出 5 份主架构文档（高层/系统/UserStory/部署/安全）。
- **含缺失能力设计**：审计发现"上传仓库初始化（upload-repo-init）"能力在代码中不存在；本次将其作为**待设计能力**纳入架构方案（状态机/幂等/回滚/安全校验），复用既有的 `resolve_sandboxed` / `ssrf_guard_resolve` / `restrict_file_permissions` / `shell` 双层门禁等安全基件。

## 3. 模板映射表（注入路径）
| 成员 | 主文档 | 模板路径 | 输出路径 | Gate |
|------|--------|---------|---------|------|
| knowledge-ingest-engineer | 资料摘要 | `...\templates\material_digest.md` | `.workbuddy/output/material_digest.md` | G1 |
| research-analyst | 调研报告 | `...\templates\research_report.md` | `.workbuddy/output/research_report.md` | G2 |
| business-architect | 高层架构设计 | `...\templates\高层架构设计.md` | `.workbuddy/output/高层架构设计.md` | G3 |
| system-architect | 系统设计 | `...\templates\系统设计.md` | `.workbuddy/output/系统设计.md` | G4 |
| product-story-designer | UserStory | `...\templates\UserStory.md` | `.workbuddy/output/UserStory.md` | G4 |
| platform-architect | 部署设计 | `...\templates\部署设计.md` | `.workbuddy/output/部署设计.md` | G5 |
| security-architect | 安全设计 | `...\templates\安全设计.md` | `.workbuddy/output/安全设计.md` | G5 |

模板根目录：`C:\Users\Administrator\.workbuddy\plugins\marketplaces\experts\plugins\aicoding-architecture-expert-team\skills\aicoding-team-bootstrap\templates\`
校验脚本：`...\bin\validate_template_compliance.py --output-dir .workbuddy/output --filter <file>`

## 4. 阶段门表（Gate）
| Gate | 名称 | 通过条件 |
|------|------|---------|
| G0 | 启动确认 | Team 创建；运行时决策/模板映射/术语表/Owner/输出目录明确 |
| G1 | 资料摘要审核 | material_digest.md 校验通过 + 人工审核通过 |
| G2 | 调研报告审核 | research_report.md 校验通过 + 人工审核通过 |
| G3 | 高层架构审核 | 高层架构设计.md 校验通过 + 人工审核通过 |
| G4 | 中游设计审核 | 系统设计.md + UserStory.md 均校验通过 + 人工审核通过 |
| G5 | 下游设计审核 | 部署设计.md + 安全设计.md 均校验通过 + 交叉一致性 diff 通过 + 人工审核通过 |
| G6 | 全量交付审核 | 术语统一/引用一致/冲突裁决完成 + 人工审核确认可归档 |

## 5. 主文档 Owner 表
| 文档 | Owner | 禁止越权范围 |
|------|-------|------------|
| material_digest.md | knowledge-ingest-engineer | 仅摘要整理，不做方案设计 |
| research_report.md | research-analyst | 仅调研对比，不冻结业务边界 |
| 高层架构设计.md | business-architect | 冻结业务边界/高层架构/MVP |
| 系统设计.md | system-architect | 模块拆分/接口契约/数据/可观测 |
| UserStory.md | product-story-designer | 角色场景/验收标准/非功能需求 |
| 部署设计.md | platform-architect | 部署/资源拓扑/CI-CD/容量成本 |
| 安全设计.md | security-architect | 威胁模型/IAM/数据/运行时防护 |

## 6. 术语表（统一口径，合稿时校验）
| 术语 | 含义 |
|------|------|
| ganyu-agent | Rust CLI AI agent，本方案目标系统 |
| Unit / RunContext / Workflow | 工作流引擎原子基底（Unit）、共享上下文（RunContext）、6 种协调策略（Workflow） |
| LlmBackend / Memory / Tool / Gateway | 核心抽象 trait |
| nomifun | 随附 33 项能力包（skills/nomifun），非 agent 核心代码 |
| 上传仓库初始化 (upload-repo-init) | 审计发现的缺失能力，本次需设计（仓库 clone/checkout/commit/push 的初始化流程） |
| fail-closed | 失败闭环安全哲学：任一校验失败即拒绝，默认拒绝 |
| hardened | Cargo feature 组合（network/crypto/secret/shell，审计建议补 sandbox） |
| R-1~R-9 | 残余风险登记项；R-6/R-8/R-9 已于第三阶段加固闭环 |
| F-01~F-14 | 安全发现项（SECURITY-REPORT.md） |
| ADR-001~008 | 现有架构决策记录 |
| Landlock | Linux 进程级 FS 沙箱（sandbox feature，仅 Linux） |

## 7. 上游原始资料（Phase 1 摄入源）
- 工程审计报告：`deliverables/engineering-assurance/full-audit-ganyu-agent-2026-08-18.md`
- 安全文档：`docs/SECURITY-REPORT.md`、`docs/security_fixes.md`、`SECURITY.md`、`docs/update-signing.md`
- 架构文档：`docs/architecture.md`、`docs/ADR-*.md`（8 篇）、`docs/build-cache-plan.md`
- 部署/使用：`docs/install.md`、`docs/config-guide.md`、`docs/usage.md`、`docs/development.md`、`install.sh`、`install.ps1`、`.github/workflows/release.yml`、`scripts/sign-release.py`
- 代码：`src/**/*.rs`（34 文件，约 7.6k LOC）
- 能力：`skills/nomifun/*`、`plugins/example.json`

---
G0 已宣布通过。下游阶段产物均落盘 `.workbuddy/output/`，最终由 team-lead 汇总归档 `delivery/`。
