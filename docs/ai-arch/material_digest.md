# AICoding 架构设计 · 资料摘要

> 本文档做一件事：**精读主理人转交的全部原始资料，逐份、逐章节做出摘要**——后面任何人拿到这份摘要，都能通过章节号快速定位回原始文件的对应位置。

> 上游输入：主理人转交的全部原始资料（工程审计报告、安全/架构文档、ADR、部署/使用/工程化文档、安装与签名脚本、能力清单、示例插件、以及作为事实溯源的源码重点文件）；
> 产出者：`knowledge-ingest-engineer`（知识摄入工程师 - 闻资料），经 G1 校验与人工审核通过后交付。

---

## 0. 元信息

```yaml
标题: ganyu-agent - 资料摘要 v0.1
版本: v0.1
状态: Draft（待 G1 校验与人工审核）
创建日期: 2026-08-18
整理人: knowledge-ingest-engineer（闻资料）
审核人:
  - 主理人 / team-lead（G1 人工审核待执行）

原始资料清单:
  - D1 deliverables/engineering-assurance/full-audit-ganyu-agent-2026-08-18.md: 9 维度工程审计报告（总体 🟡 有条件通过 B+）
  - D2 docs/SECURITY-REPORT.md: Phase-2 STRIDE 威胁建模 + R-1~R-9 残余风险处置
  - D3 docs/security_fixes.md: F-01~F-14 与 HARD-1~3/R-1、R-6/R-8/R-9 修复明细
  - D4 SECURITY.md: 威胁模型、12 层防御表、ed25519 公钥、诚实边界
  - D5 docs/update-signing.md: ed25519 签名契约、公钥轮换、安装信任锚
  - D6 docs/architecture.md: 架构分层、核心抽象、Workflow 多范式、模块职责
  - D7 docs/ADR-001~ADR-008（8 篇）: 架构决策记录（全部 Accepted）
  - D8 docs/build-cache-plan.md: 构建缓存方案 A/B/C 与 B2 profile
  - D9 docs/install.md: 安装部署文档
  - D10 docs/config-guide.md: 配置指南
  - D11 docs/usage.md: 使用文档
  - D12 docs/development.md: 开发文档
  - D13 install.sh: 一键安装脚本（Linux/macOS）
  - D14 install.ps1: 一键安装脚本（Windows）
  - D15 .github/workflows/release.yml: CI 发布流程
  - D16 scripts/sign-release.py: Ed25519 签名/校验脚本
  - D17 docs/nomifun_capabilities.md + skills/nomifun/*: 33 项 nomifun 能力清单
  - D18 plugins/example.json + 配套示例插件脚本: 最小可用插件样例
  - SRC src/*.rs（34 个，重点 security/sandbox/config/cache/core/memory/core/unit/core/workflow/*/routing/mod/main/ext/knowledge/sag/knowledge/mdl/heal）: 作为 D1 审计发现的事实溯源抽查核验
```

| 版本 | 日期 | 作者 | 变更内容 |
| --- | --- | --- | --- |
| v0.1 | 2026-08-18 | knowledge-ingest-engineer | 初稿（G1 待校验） |

---

## 1. 资料清单

> 列出全部原始资料，每份标注解析状态。解析失败或跳过的必须注明原因。

| 编号 | 文件名 | 类型 | 来源 | 解析状态 | 说明 |
| --- | --- | --- | --- | --- | --- |
| D1 | `deliverables/engineering-assurance/full-audit-ganyu-agent-2026-08-18.md` | markdown | 工程保障交付物 | 已解析 | 9 维度审计，核心输入 |
| D2 | `docs/SECURITY-REPORT.md` | markdown | 项目仓库 | 已解析 | Phase-2 STRIDE + R-1~R-9 |
| D3 | `docs/security_fixes.md` | markdown | 项目仓库 | 已解析 | F-01~F-14 / HARD / R 系列修复明细 |
| D4 | `SECURITY.md` | markdown | 项目仓库 | 已解析 | 安全总览与诚实边界 |
| D5 | `docs/update-signing.md` | markdown | 项目仓库 | 已解析 | 更新签名契约 |
| D6 | `docs/architecture.md` | markdown | 项目仓库 | 已解析 | 架构说明 |
| D7 | `docs/ADR-001.md` ~ `docs/ADR-008.md` | markdown（8 篇） | 项目仓库 | 已解析 | 架构决策记录，全部 Accepted |
| D8 | `docs/build-cache-plan.md` | markdown | 项目仓库 | 已解析 | 构建缓存方案 |
| D9 | `docs/install.md` | markdown | 项目仓库 | 已解析 | 安装部署 |
| D10 | `docs/config-guide.md` | markdown | 项目仓库 | 已解析 | 配置指南 |
| D11 | `docs/usage.md` | markdown | 项目仓库 | 已解析 | 使用文档 |
| D12 | `docs/development.md` | markdown | 项目仓库 | 已解析 | 开发文档 |
| D13 | `install.sh` | shell | 项目仓库 | 已解析 | 一键安装（POSIX） |
| D14 | `install.ps1` | powershell | 项目仓库 | 已解析 | 一键安装（Windows） |
| D15 | `.github/workflows/release.yml` | yaml | 项目仓库 | 已解析 | CI 发布流程 |
| D16 | `scripts/sign-release.py` | python | 项目仓库 | 已解析 | Ed25519 签名脚本 |
| D17 | `docs/nomifun_capabilities.md` + `skills/nomifun/*` | markdown + 目录 | 项目仓库 | 已解析 | 33 项 nomifun 能力清单与实现目录 |
| D18 | `plugins/example.json` + 配套示例插件脚本（`plugins/upper.py`，配置中写作 `python plugins/upper.py`） | json + python | 项目仓库 | 已解析 | 最小可用插件样例（`vetted:true`） |
| SRC | `src/*.rs`（34 个重点文件） | rust | 项目仓库 | 已抽查核验 | 作为 D1 审计发现的事实溯源，非独立交付物；用于交叉验证 Top-18 与债务项 |

**类型枚举**：本次资料均为 Markdown / 源码（Rust）/ 脚本（Shell/PowerShell/Python）/ 配置（YAML/JSON/TOML）类文本，**未包含** `docx` / `pdf` / `pptx` / `xlsx`（详见附录 B）。

---

## 2. 资料内容摘要

> 逐份文档按自身章节结构做摘要。每条摘要标注章节号（`D编号，§章节`），后面任何人想核实某个点，直接定位回原文对应位置即可。

### D1：`deliverables/engineering-assurance/full-audit-ganyu-agent-2026-08-18.md`

> 9 维度工程审计报告（Cody/Archi/Rex/Tessa/Docu 五位视角），总体结论 🟡 有条件通过（B+），并给出 Top-18 代码发现、运维就绪度缺口、技术债与缺失能力清单。— 来源：工程保障交付物

| 章节 | 内容摘要 |
| --- | --- |
| §1 总体结论 | 总体 🟡 有条件通过（评级 B+）；代码严重度：🔴 0 / 🟠 4 / 🟡 ~25 / 🟢 ~6；运维就绪度存在 4 项 🔴 缺口。 |
| §2 代码维度严重度 | 🔴 严重 0 项；🟠 高 4 项（见 §3 Top-18 #1~#4）；🟡 中约 25 项；🟢 低约 6 项。 |
| §3 代码维度 Top-18 发现 | #1 🟠 `OpenVikingMemory::client()`（`core/memory.rs:399-404`）每次调用新建 `reqwest::Client`，连接池失效；#2 🟠 `routing/mod.rs:154` `breakers.get(&name).unwrap()` 在热重载/`complete()` 路径可能 panic；#3 🟠 `core/memory.rs:98-133` `LocalMemory` 每次 commit 全量重写 JSON 且为阻塞 IO；#4 🟠 `main.rs:703,716` `tmp.to_str().unwrap()` 非 UTF-8 路径触发 panic；#5~#18 中/低（正则反复编译、阻塞 DNS、`is_safe_program` 允许 `/`、LLM 缓存键 `unwrap_or_default`、`board_set` 全扫描、Windows `USERNAME` 为空时 fail-soft、魔法数字、双份 hex 实现等）。（以上 #1/#2/#3/#4/#5/#9/#13/#18 与 SRC 源码抽查一致，已核实。） |
| §4 运维就绪度缺口（4 项 🔴） | ①无 supervisor/看门狗；②无可观测性（指标/日志/trace）；③升级非原子（无 `.old` 兜底与回滚）；④审计日志无轮转策略。 |
| §5 技术债清单 | F1：`hardened` feature 缺失 `sandbox`（优先级 30，已知债）；A3：`LocalReasoner` 硬编码；M1/M2/M3/M4：记忆/自愈/缓存/路由模块 TODO；U1：`upload`/`update` 上帝模块；A2：`core`↔`ext` 耦合；A1：`main.rs` 1248 行上帝模块。 |
| §6 缺失能力「上传仓库初始化」 | 经 5 位专家与源码核验，**确认代码中不存在**「上传仓库初始化」能力；D6 架构与 D17 能力清单均未包含该项。标注为**缺失能力**，需主理人/下游确认是否应在范围内。 |
| §7 行动项（20 项） | 含 3 项 P0（与 §3 #1/#2/#3 高优修复对应）及若干 P1/P2/P3；其中「upload-repo 范围」要求用户（主理人）确认 scope——列为**待外部验证点**。 |

> 事实溯源（SRC 抽查核验，支撑 §3/#1~#4/#5/#9/#13/#18 与 §5/F1）：`src/core/memory.rs`（`OpenVikingMemory::client()` 每次 `reqwest::Client::builder().timeout(5s).build()`；`LocalMemory` 全量写入）、`src/routing/mod.rs`（热重载原子 swap；`complete()` 内 `breakers.get(&name).unwrap()`）、`src/security.rs`（`restrict_file_permissions` 在 Windows `USERNAME` 为空时返回 false fail-soft；`is_safe_program` 拒绝 `..`/shell/绝对路径/含 `:` 但允许名字中含 `/`）、`src/cache.rs`（`LruCache` 用 `Vec::position()` 线性扫描）、`src/knowledge/mdl.rs`（`detect_injection` 循环内 `Regex::new` 重编译）、`Cargo.toml`（`default=["crypto","secret"]`；`hardened=["network","crypto","secret","shell"]` 缺失 `sandbox`，即债务 F1；`panic="abort"`；B2 profile）。

### D2：`docs/SECURITY-REPORT.md`

> Phase-2 STRIDE 威胁建模报告，登记 R-1~R-9 残余风险并给出处置状态。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §0 摘要 | 声明 F-01~F-14 全部闭环；R-1/R-2/R-5/R-7 已加固；R-3/R-4/R-6/R-8/R-9 **接受残余**。 |
| §1 风险总览 | 与 §0 一致，列出 R-1~R-9 处置结论（部分标注「接受残余」）。 |
| §2 STRIDE 分析 | 按 Spoofing/Tampering/Repudiation/InfoDisclosure/Dos/Elevation 逐威胁给出缓解与残余说明。 |
| §3 已闭环项 | 列出 F-01~F-14 闭环证据；并提及 R-6/R-8/R-9 **已第三阶段加固闭环**（与 §0/§1 表述不一致，见 §3 冲突 X1）。 |
| §4 残余风险明细 | R-1~R-9 逐项：R-1/R-2/R-5/R-7 加固；R-3/R-4/R-6/R-8/R-9 接受残余。§4.5 又注明 R-6/R-8/R-9 已在第三阶段加固闭环（与 §0/§1 矛盾，见 X1）。 |

### D3：`docs/security_fixes.md`

> 安全修复明细，逐条列出 F-01~F-14、HARD-1~3/R-1，以及 R-6/R-8/R-9 的加固细节。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 F 系列闭环 | F-01~F-14 每项对应代码位置与修复方式（含 `resolve_sandboxed`、`ssrf_guard_resolve`、`shell_allowed`、`sanitize_model_output`、`restrict_file_permissions`、`is_safe_archive_entry`、`fence_untrusted`、`decode_hex`、`is_safe_program` 等控制点）。 |
| §2 HARD / R-1 | HARD-1~3 与 R-1（ed25519 供应链签名）互操作测试细节。 |
| §3 R-6/R-8/R-9 加固 | 给出 R-6/R-8/R-9 的第三阶段加固实现（与 D2 §0/§1「接受残余」表述冲突，见 X1）。 |
| §4 测试计数声明 | 声明「55 测试（42 单元 + 5 集成 + 8 工作流）」。（与审计口径冲突，见 §3 冲突 X2） |

### D4：`SECURITY.md`

> 项目安全总览：威胁模型、12 层防御、ed25519 公钥与诚实边界。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 威胁模型 | 不可信输入为默认假设；default-deny、显式 opt-in；失败闭环（fail-closed）哲学。 |
| §2 12 层防御 | Hermes 8 层 + ganyu 12 层防御矩阵表（含 FS 沙箱、SSRF 防护、shell 双层校验、模型输出消毒、文件权限收紧、归档条目校验、不可信围栏、hex 解码、安全程序判定等控制点）。 |
| §3 ed25519 公钥 | 公开签名公钥 `d2de2259cce226840e7acb743b89b98cf603d2781e7b1b5456855efe8bf02cec`（轮换于 2026-08-18，见 D5）。 |
| §4 诚实边界 | 明确 sandbox ≠ 容器隔离；Landlock 仅 Linux 生效，其余平台为安全 no-op（与 D6/D8 中「沙箱」表述需下游区分）。 |

### D5：`docs/update-signing.md`

> 更新签名契约与密钥轮换流程说明。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 签名契约 | Ed25519（RFC 8032）对发布产物签名；`sign-release.py` ↔ `main.rs::verify_update_signature`（ring 0.17）互操作。 |
| §2 公钥轮换 | 公钥于 2026-08-18 轮换；新公钥即 D4 §3 所示值。 |
| §3 安装信任锚 | `install.sh`/`install.ps1` 以硬编码公钥作为信任锚校验下载产物（fail-closed）。 |
| §4 轮换同步点 | 密钥轮换需同步 3 处：签名脚本常量、安装脚本常量、文档公示值（D4/D5 需一致）。 |

### D6：`docs/architecture.md`

> 架构说明：分层、核心抽象、Workflow 多范式、模块职责。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 范式对齐 | 4 类范式对齐说明（与 D7 ADR-002 多范式决策一致）。 |
| §2 分层 | 层次化结构：core / ext / security / cache / heal / routing / knowledge / sandbox。 |
| §3 核心抽象 | `Unit` × `RunContext` × `Workflow` trait 三大核心抽象。 |
| §4 工作流引擎 | 标注「Workflow（7 范式）」；代码侧为 6 个 workflow 模块（single 承载 single+ReAct 两个范式，合计 7 范式）——口径差异见 §3 冲突 X3。 |
| §5 模块职责 | 各模块职责边界说明（与 D1 §5 债务 A2 core↔ext 耦合、A1 main.rs 上帝模块形成对照）。 |

### D7：架构决策记录（ADR-001 ~ ADR-008，共 8 篇）

> 8 篇 ADR，状态均为 Accepted，覆盖架构基线、多范式、缺陷/漏洞审计、基准、整改、结构化工程缓存、安装分发、开箱即用。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| ADR-001 架构基线 | 确立分层与核心抽象（与 D6 一致）；Status: Accepted。 |
| ADR-002 多范式 | 采用多范式工作流引擎（对应 D6 §4 的 7 范式）；Status: Accepted。 |
| ADR-003 缺陷/漏洞审计 | 审计结果：Critical×5 / High×3 / Medium×6 / Low×2；Status: Accepted。 |
| ADR-004 2026 基准 | 2026 年基准与验收口径；Status: Accepted。 |
| ADR-005 P0-P3 整改 | 分级整改计划（对应 D1 §7 行动项）；Status: Accepted。 |
| ADR-006 结构化工程缓存 | 结构化工程缓存方案（对应 D8）；Status: Accepted。 |
| ADR-007 安装分发 | 安装与分发策略（对应 D9/D13/D14/D15）；Status: Accepted。 |
| ADR-008 开箱即用 | 开箱即用目标（对应 D18 示例插件、D17 能力注册）；Status: Accepted。 |

### D8：`docs/build-cache-plan.md`

> 构建缓存方案 A/B/C 与 B2 profile 设定。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 缓存方案 | 构建缓存 A（依赖）/B（产物）/C（增量）三档方案。 |
| §2 B2 profile | `profile.release` 采用 B2：thin LTO、`codegen-units=4`、`strip=true`、`panic="abort"`（与 SRC `Cargo.toml` 一致）。 |

### D9：`docs/install.md`

> 安装部署文档（覆盖 install.sh / install.ps1 与前置条件）。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 前置条件 |  Rust 工具链、网络访问、信任锚校验说明。 |
| §2 一键安装 | 指向 D13/D14 脚本；ed25519 校验、统一构建缓存、锁自愈。 |
| §3 验证 | 安装后自检与版本确认。 |

### D10：`docs/config-guide.md`

> 配置指南（环境变量与 `GANYU_*` 体系）。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 环境变量 | `GANYU_*` 环境变量全集（`ENV_DOCS`）、`security_baseline`。 |
| §2 安全基线 | `from_env` 默认值失败闭环（与 D4 §1 哲学一致）；`write_model_config`/`write_gateway_token` 调用 `restrict_file_permissions`（与 D3 §1 F 系列对应）。 |
| §3 插件配置 | 插件清单格式（与 D18 `example.json` 的 `vetted`、`python plugins/upper.py` 对应）。 |

### D11：`docs/usage.md`

> 使用文档（面向终端用户的能力与命令）。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 能力入口 | 由 `SkillBook::match_intent`（11 规则 + nomifun）派发；33 项 nomifun 能力见 D17。 |
| §2 工作流 | 7 范式用法（与 D6 §4 口径一致）。 |
| §3 限制 | 明确未包含的能力边界（未提及「上传仓库初始化」——佐证 D1 §6 缺失）。 |

### D12：`docs/development.md`

> 开发文档（构建、测试、特性门控、贡献）。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 构建与特性门控 | `default=["crypto","secret"]`；`network`/`shell`/`sandbox` 显式 opt-in；`hardened=["network","crypto","secret","shell"]`（缺失 `sandbox`，即 D1 §5 债务 F1）。 |
| §2 测试 | 测试组织方式（单元/集成/工作流），与 D3 §4「55 测试」口径相关，见 X2。 |
| §3 贡献约定 | 模块边界与 PR 约定（与 D1 §5 A1/A2 债务相关）。 |

### D13：`install.sh`

> 一键安装脚本（POSIX，Linux/macOS）。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 下载与校验 | 拉取产物并以硬编码 ed25519 公钥校验（fail-closed，对应 D5 §3）。 |
| §2 构建缓存 | 统一构建缓存（对应 D8）。 |
| §3 锁自愈 | 下载锁/版本锁异常时自愈重试。 |

### D14：`install.ps1`

> 一键安装脚本（Windows PowerShell）。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 下载与校验 | 同 D13，以硬编码 ed25519 公钥校验。 |
| §2 构建缓存 | 统一构建缓存（对应 D8）。 |
| §3 锁自愈 | 锁异常自愈重试（与 D13 对等）。 |

### D15：`.github/workflows/release.yml`

> CI 发布流程（tag 触发，3 平台 hardened 构建）。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 触发与构建 | tag 触发；3 平台 `hardened` feature 构建；`cargo test`、`cargo-audit`。 |
| §2 归档/校验和/签名 | Archive + Checksum + Ed25519 签名（secret 缺失则 fail-closed，对应 D5/D16）。 |
| §3 发布 | 创建/更新 GitHub Release 并附产物与签名。 |

### D16：`scripts/sign-release.py`

> Ed25519 签名/校验脚本（RFC 8032，fail-closed）。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 生成/签名 | 密钥生成与产物签名；失败即退出（fail-closed）。 |
| §2 公钥公示 | 导出公钥常量（需与 D4 §3、D5 §4 三处同步）。 |
| §3 校验 | 提供 verify 子命令，供 CI 与安装脚本复用。 |

### D17：`docs/nomifun_capabilities.md` + `skills/nomifun/*`

> 33 项 nomifun 能力清单与实现目录，注册形式为 `skill:NAME`（NAME 即能力名）。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 能力总览 | 33 项 nomifun 能力，注册形式为 `skill:NAME`（NAME 即能力名），实现于 `src/ext/nomifun_caps.rs`。 |
| §2 规模与用途 | 概述各项能力用途与目录体积（33 个 `skills/nomifun/*` 目录）。 |
| §3 能力边界 | 清单中**不含**「上传仓库初始化」能力（佐证 D1 §6 缺失）。 |

### D18：`plugins/example.json` + 配套示例插件脚本

> 最小可用插件样例（`vetted:true`，脚本写作 `python plugins/upper.py`）。— 来源：项目仓库

| 章节 | 内容摘要 |
| --- | --- |
| §1 清单格式 | `plugins/example.json`：声明 `vetted:true`、执行 `python plugins/upper.py` 的极简插件清单。 |
| §2 扩展机制 | 印证 D10 §3 插件配置与 D7 ADR-008 开箱即用目标的可扩展性入口。 |

---

## 3. 冲突记录

> 不同资料对同一事实描述矛盾时，**并列保留两个版本**，不做裁决。

| 编号 | 冲突主题 | 版本 A | 出处 A | 版本 B | 出处 B | 差异说明 |
| --- | --- | --- | --- | --- | --- | --- |
| X1 | R-6 / R-8 / R-9 处置状态 | 「接受残余」（字面声明 R-6/R-8/R-9 为接受残余风险） | D2，§0 摘要；D2，§1 风险总览；D2，§4 残余风险明细 | 「已第三阶段加固闭环」 | D2，§3 已闭环项；D2，§4.5；D3，§3 R-6/R-8/R-9 加固 | 同一文档（D2）内部前后表述不一致，且 D3 以「加固」口径给出实现细节。属同一事实的两种结论，**并列保留，待主理人/下游裁决**。 |
| X2 | 测试计数「55」的口径 | 「55 测试（42 单元 + 5 集成 + 8 工作流）」作为已覆盖声明 | D3，§4 测试计数声明 | 该计数来自 `cargo test --features hardened`；其中 R-1 签名互操作测试为 `network` feature 门控，**默认 feature 下不编译/不运行** | D1（审计 Docu 维度 P1-6 备注）；D12，§2 测试 | 「55」是否为默认构建下的有效计数存疑；R-1 互操作测试在默认 feature 下不执行。**并列保留，待主理人确认默认构建测试基线**。 |
| X3 | 工作流「范式数」表述 | 「Workflow（7 范式）」 | D6，§4 工作流引擎 | 代码侧为 6 个 workflow 模块（single/plan_execute/multi_agent/router/blackboard/graph） | D6，§4；SRC `src/core/workflow/mod.rs` | 非事实矛盾，属口径差异：6 个模块实现 7 个范式（single 模块同时承载 single 与 ReAct）。**列为口径说明，待下游业务架构阶段统一术语**，不计入裁决型冲突。 |

---

## 4. 硬指标清单

| 章节 | 硬指标 | 状态 |
| --- | --- | --- |
| §1 | 每份资料有解析状态，失败/跳过注明原因 | ✅（D1–D18 与 SRC 均标注「已解析/已抽查核验」，无失败/跳过） |
| §2 | 每份文档按章节逐条摘要，每条标注了 `D编号，§章节` | ✅（D1–D18 均按自身章节结构逐条摘要并标注出处） |
| §3 | 冲突信息并列保留，不做裁决 | ✅（X1/X2 并列保留双版本；X3 标注为口径差异，均未裁决） |
| 全文 | 无残留未替换的占位标记（角括号占位、示例前缀、待填日期、待补充标记） | ✅（已逐项替换填充，无残留） |
| §2 | 缺失能力与待外部验证点已显式标注 | ✅（见下「缺失与待外部验证点汇总」） |

### 缺失能力与待外部验证点汇总

- **缺失能力 ·「上传仓库初始化」**：经 D1 §6 五位专家判定与 SRC 源码核验，以及 D6 §3/D11 §3/D17 §3 能力清单交叉验证，**代码中不存在该能力**。标注为缺失能力，是否纳入范围待主理人/下游确认。
- **待外部验证 · upload-repo 范围**：D1 §7 行动项要求用户（主理人）确认 upload-repo 的能力 scope，尚未给出，列为待外部验证点。
- **待外部验证 · R-1 互操作测试编译条件**：R-1 签名互操作测试受 `network` feature 门控（见 X2），默认 feature 下不运行；其真实覆盖状态待主理人/下游澄清测试基线。
- **事实 · sandbox 平台差异**：D4 §4 诚实边界明确 Landlock 仅 Linux 生效、其余平台为安全 no-op；与 D6/D8「沙箱」表述需下游区分（属事实标注，非冲突）。

---

## 附录 A：生成流程

### 流程总览

| 步骤 | 动作 | 落入章节 |
| --- | --- | --- |
| Step0 | 读取模板（`templates/material_digest.md`）+ 主理人转交的全部原始资料（D1–D18 及 SRC 重点源码） | — |
| Step1 | 盘点资料清单，标注解析状态 | §1 |
| Step2 | 逐份打开资料，按自身章节结构逐条摘要并标注 `D编号，§章节` | §2 |
| Step3 | 交叉比对不同资料，发现并记录矛盾（X1/X2/X3）与缺失/待验证点 | §3 + §4 缺失汇总 |
| Step4 | 逐项核验硬指标（解析状态、章节标注、冲突并列、无占位符） | §4 |

```mermaid
flowchart LR
    S0[读取模板与资料] --> S1[盘点资料清单]
    S1 --> S2[逐份精读逐章节摘要]
    S2 --> S3[交叉比对记录冲突与缺失]
    S3 --> S4[硬指标自检]
```

### 整理原则

1. **逐份精读，不跨文档归并**：摘要按文档自身章节结构组织，不做跨文档的主题重组（那是下游的事）。
2. **出处即章节号**：每条摘要标注 `D编号，§章节`，直接映射回原文位置；高优代码发现均经 SRC 源码抽查核验。
3. **冲突保留**：矛盾信息并列保留两个版本（X1/X2），不擅自裁决；口径差异（X3）单独标注。
4. **事实驱动**：以原始资料中的事实为准，不添加主观推断、不做业务/技术判断与最终决策。
5. **缺失显式标注**：缺失能力与待外部验证点集中汇总，避免下游误判为「已实现」。

---

## 附录 B：解析 Skill

- `docx`：Word 类产品/业务文档 —— **本次未包含**。
- `pdf`：PDF 类规范、手册、报告 —— **本次未包含**。
- `pptx`：PPT 类方案/汇报 —— **本次未包含**。
- `xlsx`：Excel 类数据清单、指标表 —— **本次未包含**。

> 本次 18 份资料均为 Markdown / Rust 源码 / Shell·PowerShell·Python 脚本 / YAML·JSON·TOML 配置类文本，统一以**文件直读 + 结构化抽取**方式解析，未触发上述四类 Office 解析 Skill。其中 SRC 源码以「重点文件抽查核验」方式作为 D1 审计发现的事实溯源，不单独列为交付物。
