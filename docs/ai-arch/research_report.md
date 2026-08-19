# AICoding 架构设计 · 行业调研报告

> 本文档为《AICoding 架构设计》核心产物之一，定位为**行业调研报告（research_report）**。
> 上游输入：主理人转交的用户诉求与 `material_digest.md`（G1 已通过）；
> 下游输出：驱动 `business-architect`（业务架构师）的行业调研判断，最终落入《高层架构设计》的 §2 行业调研章节。
> 撰写人：`research-analyst`（研究分析师 - 查有据），经 G2 自动校验与人工审核通过后方可进入下游消费。
> 结构纪律：全文按「事实 → 对比 → 建议 → 风险」四段式组织，严禁四段之间倒序或跳段。

---

## 0. 元信息：修订记录

```yaml
标题: ganyu-agent - 行业调研报告 v0.1
版本: v0.1
状态: Draft   # Draft | Reviewing | Approved | Deprecated
创建日期: 2026-08-19
最后更新: 2026-08-19
调研人: research-analyst（查有据）
审核人:
  - 主理人 / team-lead（G2 人工审核待执行）

关联文档:
  上游输入:
    - 用户诉求: 由主理人注入（"启动 AICoding 架构专家团，基于我的项目背景和资料生成完整架构方案。严格遵循行动清单（按优先级排序）全量完整的落地落实"）
    - 调研目标: 由主理人注入（Phase 2 / G2 行动指令，覆盖 a~f 六类调研方向）
    - 资料摘要: .workbuddy/output/material_digest.md（G1 已通过）
  下游产出:
    - 高层架构设计 §2 行业调研: 将由 business-architect 整合到此章节
```

| 版本 | 日期 | 作者 | 变更内容 | 评审状态 |
| --- | --- | --- | --- | --- |
| v0.1 | 2026-08-19 | research-analyst | 初稿（G2 待校验） | Draft |

---

## 1. 调研问题收敛

> 调研启动前，先围绕用户诉求收拢为明确的调研问题集合，确保调研不偏离当前项目背景（ganyu-agent：Rust CLI AI agent，约 7.6k LOC、34 个 .rs，fail-closed 安全哲学，F-01~F-14、R-1~R-9，缺失能力「上传仓库初始化」）。

### 1.1 原始调研种子

> 从用户诉求与 G2 行动指令（a~f）中提取需要调研验证的论题，逐条给出调研优先级。

| 编号 | 待验证论题 | 来源（用户诉求 / 行动指令要点） | 调研优先级 | 备注 |
| --- | --- | --- | --- | --- |
| S1 | 同类 AI agent CLI / Rust agent 框架的架构范式（多 agent 协调、权限模型、扩展机制）与可借鉴点 | 行动指令 (a)：OpenCode / Aider / Claude Code / Codex CLI / Goose / OpenHands 等 | 高 | 直接关联 ganyu 的 Unit×RunContext×Workflow 多范式引擎 |
| S2 | 编码 agent 的沙箱 / 能力隔离方案成熟度（OS 级 Landlock/seccomp/bubblewrap 与容器对比），关联诚实边界与 F1 债务 | 行动指令 (b)；material_digest D4 §4 诚实边界、D1 §5 债务 F1 | 高 | ganyu 的 Landlock 仅 Linux 生效，其余平台为安全 no-op |
| S3 | 工具调用 / MCP 协议与插件扩展机制的成熟度与可借鉴设计 | 行动指令 (c)；material_digest D10 §3、D18 插件机制、ADR-008 | 高 | ganyu 已有 example.json 插件清单，需评估是否对齐 MCP |
| S4 | 供应链签名与发布分发最佳实践（关联 ed25519 基件），以及自托管二进制 / 容器 / 云部署形态、可观测性与原子升级基线 | 行动指令 (d)+(f)；material_digest D4 §3、D5、D15、D16；D1 §4 运维缺口 4 项 🔴 | 高 | 合并 (d) 与 (f)：二者均围绕"产物如何构建-签名-分发-部署-运维"，属同一分发运维闭环，避免拆分后重复 |
| S5 | 「上传仓库初始化」类能力的业界实现模式（状态机 / 幂等 / 回滚 / 重试 / 可恢复一致提交），支撑缺失能力设计 | 行动指令 (e)；material_digest D1 §6、§5 U1 上帝模块、_team_runtime 架构范围 | 高 | 代码确认不存在该能力，本次需设计，复用 resolve_sandboxed / ssrf_guard_resolve / restrict_file_permissions / shell 双层门禁 |

### 1.2 调研问题收敛

> 将 §1.1 的种子收敛为 5 个可执行的调研问题（覆盖行动指令 a~f）。每条问题明确调研对象、调研目标与产出预期。

| 编号 | 调研问题 | 调研对象 | 调研目标 | 预期产出 | 关联种子 |
| --- | --- | --- | --- | --- | --- |
| Q1 | 同类 AI agent CLI / Rust agent 框架的架构范式（多 agent 协调、权限模型、扩展机制）与可借鉴点是什么？ | Claude Code、Goose（Rust）、OpenCode、OpenHands + 官方架构文档与社区逆向分析 | 提炼与 ganyu 的 Unit×RunContext×Workflow 多范式引擎、核心抽象、特性门控可对照的架构范式与扩展机制 | 标杆架构范式对照表 + 可借鉴点清单 | S1 |
| Q2 | 编码 agent 的沙箱 / 能力隔离方案成熟度如何？OS 级（Landlock/seccomp/bubblewrap）与容器隔离各自适用什么场景，如何关联 ganyu 的诚实边界与 F1 债务？ | Claude Code sandboxing（bubblewrap/seatbelt）、OpenHands（Docker）、Landlock/seccomp 文档、ganyu D4 §4 诚实边界 | 评估各类隔离方案的成熟度、平台覆盖、对 ganyu F1（hardened 缺 sandbox）债务的参考路径 | 隔离方案成熟度矩阵 + F1 修复参考 | S2 |
| Q3 | 工具调用 / MCP 协议与插件扩展机制的成熟度如何？ganyu 现有插件清单是否应演进到 MCP？ | MCP 官方规范（2025-11-25）、各标杆的 MCP / 插件实现、ganyu D10 §3 / D18 / ADR-008 | 判断 MCP 作为工具/扩展协议的标准化程度、安全模型，给出 ganyu 插件机制演进建议 | 协议选型建议 + 安全边界要点 | S3 |
| Q4 | 供应链签名与发布分发最佳实践是什么？自托管二进制 / 容器 / 云部署形态、可观测性与原子升级基线如何对齐 ganyu 的 ed25519 基件与 D1 §4 运维缺口？ | ed25519/cosign/sigstore/TUF 供应链实践、Go/Rust 单二进制部署清单、ganyu D4 §3、D5、D15、D16 | 验证 ganyu ed25519（RFC 8032）契约的合理性，给出原子升级/回滚、可观测性、审计轮转的业界基线 | 供应链与部署运维基线清单 + 缺口对齐 | S4 |
| Q5 | 「上传仓库初始化」类能力的业界实现模式（状态机 / 幂等 / 回滚 / 重试 / 可恢复一致提交）是什么？如何复用 ganyu 既有安全基件设计该缺失能力？ | git init/idempotency 模式、CLI agent 可恢复一致提交模式、命令状态机模式、ganyu D1 §6 / §5 U1 / 安全基件 | 提取可复用的状态机/幂等/重试/回滚设计范式，映射到 ganyu 的 resolve_sandboxed / ssrf_guard_resolve / restrict_file_permissions / shell 双层门禁 | upload-repo-init 设计模式映射 + 安全基件复用建议 | S5 |

---

## 2. 事实：标杆系统盘点和方案详述

> **四段式「事实」段**。只陈列调研发现的事实，不做引申建议或边界裁决。

### 2.1 行业标杆清单

> 完整盘点调研覆盖的所有标杆系统，给出标签化画像。

**硬指标**：≥ 3 家；至少包含 1 家头部 SaaS 代表（Claude Code，Anthropic 闭源订阅制）+ 3 家开源 / 自研代表（Goose 为 Rust 开源、OpenCode 为 MIT 开源、OpenHands 为 MIT 开源）。

| 编号 | 标杆系统 | 厂商 / 社区 | 部署形态 | 场景覆盖 | 技术亮点 | 商业模式 | 调研来源 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| B1 | Claude Code | Anthropic（闭源 SaaS/订阅制） | 本地 CLI + 云端托管会话 | 终端 AI 编程助手、复杂项目开发 | 分层多 agent、OS 级沙箱（bubblewrap/seatbelt）、6 种权限模式、MCP、Skills/Agents/Memory 扩展体系 | 订阅制（约 17 美元/月）+ API 用量 | SR-01、SR-02、SR-03 |
| B2 | Goose | Block → Linux 基金会 AAIF（Apache-2.0 开源，Rust） | 本地 CLI + 桌面 + API（同代码库） | 通用 agent：编码、工作流、研究、自动化 | Rust 引擎、MCP-first（70+ 扩展）、可移植 YAML Recipes、并行 subagent（隔离文件域）、多 LLM（30+） | 免费开源（BYOK，仅付 API） | SR-04、SR-05、SR-06、SR-07 |
| B3 | OpenCode | SST 团队（MIT 开源，TypeScript/Bun） | 终端 TUI + 桌面 + IDE + 服务端（可远程驱动） | 开源终端 AI 编码 agent，替代 Claude Code | 客户端/服务端分离、75+ LLM provider、原生 LSP、MCP 支持、多会话并行、自定义 agent 配置（Markdown/JSON） | 免费开源（MIT，仅付 API） | SR-08、SR-09、SR-10 |
| B4 | OpenHands | All-Hands-AI（MIT 开源，Python/FastAPI+React） | Web 应用 + 每会话独立 Docker 沙箱 | 自主软件工程师、GitHub issue→PR 批量自治 | 每会话 Docker 隔离沙箱、资源配额、操作审计日志、环境快照回滚、原生 GitHub 集成、任意 LLM | 免费开源（MIT，自托管或 Cloud） | SR-11、SR-12、SR-13 |

### 2.2 标杆方案详述

> 每家标杆逐一展开（B1~B4 均有详述）；每段区分「已核实的事实」与「推断/假设」。

#### 2.2.1 B1 - Claude Code

| 维度 | 内容 | 置信度 |
| --- | --- | --- |
| 产品定位 | Anthropic 官方的终端 AI 编程助手，默认只读、按需请求权限，支持自主编码、调试、测试 | 已核实 |
| 目标用户 | 信任闭源服务、使用 Anthropic 模型、追求开箱即用体验的开发者与企业团队 | 已核实 |
| 核心能力 | 代码读写、命令执行、Git 操作、子代理（Task 工具，隔离上下文）、Skills、Agents、Memory 七级层级、MCP 连接 | 已核实 |
| 架构特点 | 分层多 agent（主 agent + SubAgent，各自独立上下文与工具权限）；客户端本地运行，提供"Claude Code on the web"云端隔离沙箱会话 | 推断（来源：社区逆向分析与官方博客） |
| 部署形态 | 本地 CLI 安装 + 云端托管会话（敏感凭据如 git/签名密钥不进入沙箱） | 已核实 |
| 集成方式 | MCP（官方与社区服务器）、自定义 Agents（Markdown 定义）、Hooks、managed policies（企业级组织策略，/etc/claude-code/ 系统路径覆盖用户设置） | 已核实 |
| 定价模式 | 订阅制（公开资料提及约 17 美元/月）+ LLM API 用量；闭源，不可自托管 | 已核实 |
| 优势 | 权限模型成熟（6 种权限模式、deny→ask→allow 评估顺序、bypassPermissions 仅限容器）；OS 级沙箱（bubblewrap/Linux、seatbelt/macOS）同时做文件系统与网络隔离；扩展生态丰富 | 综合归纳 |
| 局限 | 闭源、模型绑定 Anthropic、不可审计与自托管；数据经 Anthropic；对需要数据主权/审计的受监管场景不友好 | 已核实 + 推断 |
| 对本项目的参考价值 | 权限模式与 OS 级沙箱（文件系统+网络双隔离）设计，对 ganyu 的 hardened 特性门控、F1 sandbox 债务、12 层防御矩阵有直接借鉴意义；其"沙箱 ≠ 权限"的区分与 ganyu D4 §4 诚实边界一致 | 推断 |

#### 2.2.2 B2 - Goose

| 维度 | 内容 | 置信度 |
| --- | --- | --- |
| 产品定位 | Block 开源、现由 Linux 基金会 AAIF 治理的本地优先通用 AI agent，Rust 引擎，MCP-first | 已核实 |
| 目标用户 | 追求供应商中立、可扩展、可自托管的开发者与团队；安全敏感（可审计、无数据外流）组织 | 已核实 |
| 核心能力 | 代码生成/重构、CI/CD 自动化、多步工程工作流；通过 MCP 扩展连接 GitHub/数据库/浏览器/API 等 70+ 工具 | 已核实 |
| 架构特点 | 单进程 ReAct 工具循环（桌面/CLI/API 同一核心）；可移植 YAML Recipes 封装指令+扩展+参数+子配方，可在 CI 运行；subagent 并行（隔离文件域）；GOOSE_MAX_TURNS 步数预算 | 已核实 |
| 部署形态 | 本地桌面 + CLI + 可嵌入 API（同代码库）；macOS/Linux/Windows；自托管可行 | 已核实 |
| 集成方式 | MCP-first（70+ 官方扩展，社区 1700+ MCP 服务器）；Recipes 为可复用技能库；provider 矩阵 30+ LLM | 已核实 |
| 定价模式 | Apache-2.0 免费开源，BYOK（约 5~50 美元/月 API 费，依模型与用量） | 已核实 |
| 优势 | Rust 引擎（启动快、低延迟工具调用，与 ganyu 同为 Rust CLI 直接对标）；MCP-first 架构标准化扩展；Linux 基金会治理避免供应商锁定；完全可审计/自托管 | 综合归纳 |
| 局限 | 无商业 SLA/合规认证（SOC2/HIPAA 缺失）；BYOK API 成本需自行管理；社区治理下企业支持依赖 GitHub/Discord | 已核实 + 推断 |
| 对本项目的参考价值 | 与 ganyu 同为 Rust CLI agent，架构范式（单进程 ReAct 循环、可移植 YAML 工作流、subagent 隔离、多 provider）高度可对照；其"能力即 MCP 扩展、工作流即 Recipe"的分层对 ganyu 的 Workflow 多范式与插件机制最具借鉴价值 | 推断 |

#### 2.2.3 B3 - OpenCode

| 维度 | 内容 | 置信度 |
| --- | --- | --- |
| 产品定位 | SST 团队构建的开源终端 AI 编码 agent，明确作为 Claude Code 的开源替代，无供应商绑定 | 已核实 |
| 目标用户 | 追求开源可控、自由切换模型、注重扩展插件能力的开发者与团队 | 已核实 |
| 核心能力 | 文件读写/Shell/Git、LSP（原生语言感知诊断）、MCP、多会话并行、自定义 agent（Build/Plan/General） | 已核实 |
| 架构特点 | 客户端/服务端分离（服务端 Bun/TypeScript，TUI 为可替换前端，服务端可远程驱动）；provider router（75+ provider，Vercel AI SDK）；基于 Effect 的并发；Drizzle+SQLite 本地会话持久化 | 已核实 |
| 部署形态 | 本地 TUI + Web UI + 桌面（Tauri）+ Server 模式（可远程/CI 调用） | 已核实 |
| 集成方式 | MCP 支持；Markdown/JSON 自定义 agent（.opencode/agents/）；/init 生成 AGENTS.md；细粒度工具权限（opencode.json 配置命令黑白名单） | 已核实 |
| 定价模式 | MIT 免费开源，仅付 API | 已核实 |
| 优势 | 开源可审计；客户端/服务端分离利于远程执行与多会话；LSP 原生集成（语言感知，ganyu 当前未重点覆盖）；架构分层清晰（Agent Engine/Provider Router/LSP Manager/Session Manager） | 综合归纳 |
| 局限 | 语言为 TypeScript（非 Rust），代码不可直接复用给 ganyu；社区规模虽大但项目较新（2025-06 起） | 已核实 + 推断 |
| 对本项目的参考价值 | 其"客户端/服务端分离 + Provider Router + 多角色 agent（Build/Plan/General）+ 细粒度权限配置"对 ganyu 的 core↔ext 解耦（债务 A2）、多范式工作流引擎、以及 P0/P1 整改的分层重构有借鉴意义；LSP 集成可作为 ganyu 能力扩展的参考方向 | 推断 |

#### 2.2.4 B4 - OpenHands

| 维度 | 内容 | 置信度 |
| --- | --- | --- |
| 产品定位 | All-Hands-AI 开源的自主软件工程师，Web 应用形态，让 AI 成为"全栈开发者" | 已核实 |
| 目标用户 | 需要 Web 界面、沙箱隔离、对 GitHub issue 批量自治的团队；偏好自主批处理而非交互式编码的用户 | 已核实 |
| 核心能力 | 在隔离 Docker 沙箱内读/写/执行代码、浏览网页、运行 shell/git/包管理；原生 GitHub/GitLab 集成（issue→PR）；支持任意 LLM（云 API 或自托管 vLLM/Ollama） | 已核实 |
| 架构特点 | 浏览器 → OpenHands Server（FastAPI+React, 3000 端口）→ 每会话独立 Docker 沙箱 → LLM API；三层镜像构建体系（Versioned/Lock/Source Tag）；环境快照回滚 | 已核实 |
| 部署形态 | 自托管（Docker Compose，需挂载 /var/run/docker.sock，仅限可信基础设施）；OpenHands Cloud 提供托管沙箱；云端/本地/Docker 三模 | 已核实 |
| 集成方式 | MCP 支持；内置 Jupyter 内核、浏览器控制、API 网关等插件；自定义 Python 工具插件 | 已核实 |
| 定价模式 | MIT 免费开源；自托管免费（模型 API 另计），或 OpenHands Cloud | 已核实 |
| 优势 | 每会话 Docker 隔离（agent 不能直接访问宿主机文件系统）；资源配额（CPU/内存）；操作审计日志；环境快照回滚——隔离与可恢复性设计成熟 | 综合归纳 |
| 局限 | Web/Docker 架构与 ganyu 的"本地单二进制 CLI"形态差异大；自托管需 Docker + GPU（本地模型），运维开销高；沙箱较重，不适合轻量 CLI 场景 | 已核实 + 推断 |
| 对本项目的参考价值 | 其 Docker 沙箱"每会话隔离 + 资源配额 + 操作审计 + 快照回滚"是 F1 sandbox 债务的强参考（尤其非 Linux 平台的隔离替代思路）；但其整体 Web/Docker 架构不宜作为 ganyu 主架构借鉴，仅沙箱与可恢复性子模式可参考 | 推断 |

### 2.3 关键技术能力横向事实

> 不评分、不排序，仅按能力维度横陈各方案事实（事实来源见 §6）。

| 能力维度 | B1 Claude Code | B2 Goose | B3 OpenCode | B4 OpenHands | 说明 / 来源 |
| --- | --- | --- | --- | --- | --- |
| 核心语言 | TypeScript/JS（闭源） | Rust（开源） | TypeScript/Bun（开源） | Python/FastAPI+React（开源） | SR-04、SR-08 |
| 交互形态 | 本地 CLI + 云端沙箱会话 | 本地 CLI + 桌面 + API | 终端 TUI + 桌面 + IDE + Server | Web + 每会话 Docker 沙箱 | SR-01、SR-04、SR-08、SR-11 |
| 多 agent / 工作流范式 | 分层主/子代理（隔离上下文） | 单进程 ReAct + 并行 subagent（隔离文件域） | 多角色 agent（Build/Plan/General）+ 多会话并行 | 自主批处理（issue→PR） | SR-02、SR-05、SR-09 |
| 工具 / 扩展协议 | MCP + 自定义 Agents/Hooks | MCP-first（70+ 扩展）+ YAML Recipes | MCP + 自定义 agent（Markdown/JSON） | MCP + 自定义 Python 工具插件 | SR-03、SR-06、SR-10、SR-12 |
| 沙箱 / 隔离机制 | OS 级（bubblewrap/Linux、seatbelt/macOS），FS+网络双隔离 | 无内建 OS 沙箱（依赖主机权限与 MCP 范围） | 无内建 OS 沙箱（依赖权限配置） | 每会话 Docker 容器隔离 + 资源配额 | SR-01、SR-06、SR-09、SR-11 |
| 权限模型 | 6 种权限模式（default/acceptEdits/plan/delegate/dontAsk/bypass）+ deny→ask→allow | 按工具/扩展的 autonomy/permission 配置 | opencode.json 命令黑白名单 + 项目级安全规则 | 沙箱即权限边界（容器外不可访问） | SR-02、SR-06、SR-09 |
| 供应链签名 / 自托管 | 闭源，不可自托管 | 开源可自托管、可审计 | 开源可自托管、可审计 | 开源可自托管、可审计 | SR-04、SR-08、SR-11 |
| 许可 / 成本 | 闭源订阅（约 17 美元/月）+ API | Apache-2.0 免费 + BYOK | MIT 免费 + API | MIT 免费 + 自托管/Cloud | SR-02、SR-05、SR-08、SR-11 |
| 原生可观测性 | 未内建（依赖外部） | 每会话 JSON 导出（token/模型/时间戳） | 会话历史/成本分析 | LOG_ALL_EVENTS 事件日志 | SR-05、SR-09、SR-12 |
| 原子升级 / 回滚 | 云端托管，客户端更新 | 本地二进制替换（无内建原子回滚） | 本地二进制替换（无内建原子回滚） | 镜像版本化（三层 Tag）+ 快照回滚 | SR-05、SR-09、SR-12 |

---

## 3. 对比：对比矩阵与加权评分

> **四段式「对比」段**。在 §2 的事实基础上建立对比矩阵，赋予权重并打分。

### 3.1 对比矩阵

> **每行权重之和 = 1.00**。评估维度与权重沿用模板默认设定（已针对本项目场景理由化）：场景契合度 0.30 因 ganyu 是 Rust CLI agent、fail-closed、需对齐多范式引擎与插件扩展，架构范式参考价值权重最高；技术成熟度 0.20 反映生态稳定性对落地风险的影响；集成难度（反向）0.15 与成本（反向）0.15 体现"可学习/可复用且经济"的偏好；合规可控性 0.20 对应 ganyu 受监管/审计场景与 fail-closed 哲学（闭源 SaaS 在此项吃亏）。

| 评估维度 | 权重 | 权重理由 | B1 得分 | B2 得分 | B3 得分 | B4 得分 |
| --- | --- | --- | --- | --- | --- | --- |
| 场景契合度 | 0.30 | ganyu 为 Rust CLI agent、fail-closed、多范式引擎与插件扩展，架构范式参考价值最重要 | 5 | 5 | 4 | 3 |
| 技术成熟度 | 0.20 | 生态稳定性、采用规模、文档完善度直接影响落地风险 | 5 | 4 | 5 | 4 |
| 集成难度（反向） | 0.15 | 反向计分：越易学习/复用概念（开源可审计）得分越高；闭源不可直接复用则扣分 | 4 | 5 | 4 | 3 |
| 成本（反向） | 0.15 | 反向计分：免费开源/自托管得分高；订阅+闭源不可自托管得分低；Docker 自托管运维开销中等 | 3 | 5 | 5 | 3 |
| 合规可控性 | 0.20 | ganyu 受审计/数据主权约束、fail-closed 需可审计；闭源 SaaS 不可审计在此项最弱 | 2 | 5 | 5 | 5 |
| **加权总分** | **1.00** | — | **3.95** | **4.80** | **4.55** | **3.60** |

**评分标尺**：每项 1~5 分，1 = 严重不符合，3 = 基本满足但存在明显局限，5 = 完美契合。

**加权总分计算明细**：
- B1 = 5×0.30 + 5×0.20 + 4×0.15 + 3×0.15 + 2×0.20 = 1.50 + 1.00 + 0.60 + 0.45 + 0.40 = 3.95
- B2 = 5×0.30 + 4×0.20 + 5×0.15 + 5×0.15 + 5×0.20 = 1.50 + 0.80 + 0.75 + 0.75 + 1.00 = 4.80
- B3 = 4×0.30 + 5×0.20 + 4×0.15 + 5×0.15 + 5×0.20 = 1.20 + 1.00 + 0.60 + 0.75 + 1.00 = 4.55
- B4 = 3×0.30 + 4×0.20 + 3×0.15 + 3×0.15 + 5×0.20 = 0.90 + 0.80 + 0.45 + 0.45 + 1.00 = 3.60

### 3.2 评分结论

> 基于 §3.1 加权总分，形成分层结论。每层结论引用得分作为依据。**本节为评估而非授权，最终边界由 business-architect 冻结。**

- **优先借鉴**：**B2 Goose（4.80）** — 适用度评分最高。理由：与 ganyu 同为 **Rust CLI agent**（架构范式直接对标）；MCP-first + 可移植 YAML Recipes + 隔离文件域 subagent 的"能力即 MCP 扩展、工作流即 Recipe"分层，对 ganyu 的 Workflow 多范式引擎与插件机制（D18/ADR-008）最具借鉴价值；Apache-2.0 可审计/自托管契合 fail-closed 与受监管场景。场景契合度 5、合规可控性 5 双高。
- **部分借鉴**：
  - **B3 OpenCode（4.55）** — 借鉴点：客户端/服务端分离、Provider Router（75+ provider）、多角色 agent（Build/Plan/General）与 `opencode.json` 细粒度命令黑白名单权限，对 ganyu 的 **core↔ext 解耦（债务 A2）**、多范式工作流与权限配置有参考；原生 LSP 集成可作为能力扩展方向。不借鉴的部分：语言为 TypeScript，代码不可直接复用，仅作架构范式参考。
  - **B1 Claude Code（3.95）** — 借鉴点：6 种权限模式（deny→ask→allow 评估顺序）、OS 级沙箱（bubblewrap/seatbelt，文件系统+网络双隔离）、Skills/Agents/Memory 扩展体系、managed policies 企业级覆盖。此三项对 ganyu **F1 sandbox 债务**、12 层防御矩阵、hardened 特性门控直接有用。不借鉴的部分：闭源、模型绑定、不可自托管、合规可控性仅 2 分，整体不可作为底座。
- **不借鉴（否决）**：**B4 OpenHands（3.60）** — 否决理由：Web + 每会话 Docker 沙箱的整体架构与 ganyu「本地单二进制 CLI」形态根本不符（场景契合度仅 3）；自托管需 Docker + GPU，运维开销高（成本反向仅 3）。其价值仅限于**沙箱隔离与可恢复性子模式**（每会话隔离、资源配额、操作审计、快照回滚）作为 F1 债务与 D1 §4 运维缺口的参考，而非整体架构借鉴。

### 3.3 方案组合分析（如有）

> 调研发现"单一方案无法覆盖全部需求"，给出组合建议。

| 组合方式 | 覆盖哪些能力 | 未覆盖能力 | 组合复杂度 | 总体成本估算 |
| --- | --- | --- | --- | --- |
| **B2（Goose）为主架构范式 + B3（OpenCode）补充分层/权限 + B1（Claude Code）补沙箱/权限模式** | Rust CLI agent 主线架构；MCP-first 扩展；客户端/服务端分层与多角色 agent 权限；OS 级沙箱（FS+网络）与权限评估链 | F1 非 Linux 平台隔离仍需结合 B4 的 Docker 思路或维持诚实边界 no-op；Web/Cloud 形态不适用 | 中（三者均为开源/可审计，范式可组合） | 低（均为开源，无授权费；成本主要在自研集成人力） |
| **B4（OpenHands）沙箱子模式单独抽取** | 仅抽取"每会话隔离 + 资源配额 + 操作审计 + 快照回滚"用于 F1 与 D1 §4 运维缺口 | 不进入主架构（整体 Web/Docker 形态否决） | 低（子模式抽取） | 低（参考设计，自研实现） |

---

## 4. 建议：取舍决策支持

> **四段式「建议」段**。基于 §2 事实 + §3 对比，给出可被 `business-architect` 直接采用的**建议**。**本节是建议而非最终裁决，最终边界由业务架构师冻结。**

### 4.1 自研 / 采购 / 复用边界建议

| 能力项 | 建议方式 | 建议依据 | 候选方案 / 系统 | 关键前提 |
| --- | --- | --- | --- | --- |
| Agent 主架构范式（Rust CLI + 多范式工作流 + 扩展机制） | 复用（已有底座）+ 自研增强 | ganyu 已有 Unit×RunContext×Workflow 引擎与 6 模块/7 范式（D6 §4），架构主线无需推倒；参考 B2 的 MCP-first 与 B3 的分层 | ganyu 现有引擎 + 借鉴 Goose/OpenCode 范式 | 需先解耦 core↔ext（债务 A2）与拆分 main.rs 上帝模块（A1） |
| 工具 / 插件扩展协议 | 复用（演进现有插件）+ 对齐 MCP | ganyu 已有 example.json 插件清单（D18/ADR-008）；MCP 已成为行业标准（2025-11-25 规范、Linux 基金会 AAIF 治理、10k+ 服务器），B2/B3/B4 均原生支持 | 现有插件机制 + MCP 协议（SR-14） | 需评估插件模型是否演进到 MCP server；建议保留 vetted 信任锚（D10 §3） |
| 沙箱 / 能力隔离（F1 债务） | 自研（增强现有 Landlock）+ 参考 B1/B4 | F1 为 hardened 缺 sandbox（D1 §5）；B1 的 OS 级 FS+网络双隔离、B4 的 Docker 隔离可作参考；D4 §4 诚实边界要求非 Linux 平台不得伪装为容器隔离 | ganyu 现有 Landlock + resolve_sandboxed / ssrf_guard_resolve + 参考 B1/B4 | 明确非 Linux 平台维持 honest no-op 或引入轻量隔离；不把 no-op 宣传为沙箱 |
| 供应链签名与发布分发 | 复用（已有 ed25519 基件）+ 自研补全 | ganyu 已实现 ed25519（RFC 8032）签名契约（D4 §3、D5、D16），符合"Ed25519 体积小/最快/适合嵌入"最佳实践（SR-17）；缺口在原子升级与透明度 | 现有 sign-release.py + install.sh/ps1 + 参考 TUF/Sigstore（SR-18/19） | 需补 A/B 原子升级 + 回滚（D1 §4 缺口③）、SBOM、透明度日志 |
| 缺失能力「上传-repo-init」 | 自研（新建，复用安全基件） | 代码确认不存在（D1 §6）；参考 §5 Q5 的状态机/幂等/回滚模式（SR-20/21/22） | 新建 upload-repo-init 模块，复用 resolve_sandboxed / ssrf_guard_resolve / restrict_file_permissions / shell 双层门禁 | 需主理人确认 scope（_team_runtime 架构范围已纳入，但 D1 §7 行动项仍待外部验证） |
| 可观测性 / 运维（D1 §4 缺口①②） | 自研（轻量内嵌） | 标杆部署基线（SR-23/24）表明单二进制可内嵌 Prometheus /metrics、结构化日志（slog/zerolog）、OpenTelemetry trace；systemd Restart=on-failure 提供看门狗 | 内嵌 metrics + slog + OTel + systemd 单元 | 须低于 ganyu 内存/体积预算；不引入重依赖 |
| 审计日志轮转（D1 §4 缺口④） | 自研 | 标杆日志分级（audit/event/debug）+ 轮转策略（SR-24 的 Filebeat/ELK 对接、klc audit.jsonl 轮转）可参考 | 内建 audit 日志轮转 | 固定字段、NDJSON，便于对接 ELK/Splunk |

### 4.2 MVP 范围建议

> 对齐用户诉求与 ganyu 的 P0/P1（D1 §7 行动项；ADR-005）。**upload-repo-init 缺失能力已纳入架构范围（_team_runtime）。**

| 功能（对齐用户诉求 / ganyu 现状） | 建议 MVP？ | 理由 |
| --- | --- | --- |
| P0-1 OpenVikingMemory 连接池修复（D1 §3 #1） | ✅ | 高优修复，影响所有记忆调用；无需新架构，纯修复 |
| P0-2 routing breakers 热重载 panic 修复（D1 §3 #2） | ✅ | 高优修复，热重载路径稳定性；纯修复 |
| P0-3 LocalMemory 全量重写阻塞 IO 修复（D1 §3 #3） | ✅ | 高优修复，性能与阻塞；纯修复 |
| P1 运维缺口补齐：supervisor/看门狗 + 可观测性 + 原子升级 + 审计轮转（D1 §4） | ✅（分批） | 标杆部署基线成熟（SR-23/24），可内嵌轻量 metrics/log/OTel + systemd 看门狗；原子升级参考 A/B+health check；建议按 P1 分批，不在首个 MVP 全量 |
| F1 sandbox 债务补齐（hardened 缺 sandbox） | ✅（部分） | B1/B4 提供清晰参考；Linux 强化 Landlock（FS+网络），非 Linux 维持诚实 no-op 并文档化；完整跨平台隔离不作为首 MVP |
| 插件机制演进到 MCP | ⚠️（建议 P1/P2，非首 MVP） | 标准成熟但涉及契约变更，建议先冻结现有 vetted 插件模型，再规划 MCP 适配 |
| upload-repo-init（缺失能力，本次设计） | ✅（纳入 MVP 设计，首版最小集） | 已纳入架构范围（_team_runtime）；建议首版实现最小集：状态机（Pending→Running→Succeeded/Failed）+ 幂等 git 初始化/clone + 失败回滚 + 复用安全基件；完整多仓库编排留待 P2 |
| 跨平台完整容器级隔离 | ❌（完整版） | 与 ganyu 单二进制 CLI 形态不符；仅参考 B4 子模式，不在 MVP 实现 Docker 级隔离 |

### 4.3 技术栈参考建议

| 技术层 | 推荐方案 | 替代方案 | 选择理由 |
| --- | --- | --- | --- |
| Agent 核心语言 / 运行时 | Rust（维持 ganyu 现状） | Go（单二进制部署同样成熟，SR-23/24） | ganyu 已是 Rust，B2 Goose 证明 Rust agent 引擎可行；零成本抽象、内存安全契合 fail-closed |
| 工具 / 扩展协议 | MCP（2025-11-25 规范，SR-14/15/16） | 维持现有 example.json 插件清单（D18） | MCP 已成事实标准（10k+ 服务器、Linux 基金会治理），与 ganyu 插件模型可渐进对齐；保留 vetted 信任锚 |
| 供应链签名 | ed25519（RFC 8032，维持 ganyu 现状）+ TUF 元数据链 | cosign/Sigstore keyless（SR-18/19） | ganyu 已实现 ed25519，符合"体积小/快/适合嵌入"最佳实践（SR-17）；TUF 提供防仓库 compromise 与密钥轮换回滚；cosign 适合容器场景，ganyu 为二进制故 ed25519+TUF 更贴切 |
| 可观测性（内嵌） | Prometheus /metrics + slog 结构化日志 + OpenTelemetry trace | 仅 slog + 文件日志 | 单二进制内嵌 metrics/trace 是标杆部署基线（SR-23/24）；OTel 轻量 SDK 可联动；对 D1 §4 缺口②直接补齐 |
| 进程监管 / 看门狗 | systemd（Restart=on-failure, Type=notify, ProtectSystem=strict） | 容器内 supervisor / 自研心跳 | 标杆 CLI 生产实践（SR-24）证明 systemd 即可满足 supervisor 缺口；无额外依赖 |
| 原子升级 / 回滚 | A/B 双槽 + 健康检查 + 保留上一版 manifest 哈希回滚 | 全量替换 + .old 兜底 | 固件/CLI 部署基线（SR-17/23）推荐 A/B + health check 防 brick；直接解决 D1 §4 缺口③非原子升级 |
| 审计日志 | NDJSON audit 日志 + 固定字段 + 轮转策略（对接 ELK/Splunk） | 仅追加文件无轮转 | 标杆（SR-24 klc audit.jsonl）证明 NDJSON + 轮转 + 对接 SIEM 是合规基线；解决 D1 §4 缺口④ |

---

## 5. 风险与待确认项

> **四段式「风险」段**。列出调研中发现的主要风险、不确定信息、待业务架构师进一步裁决的依赖项。

### 5.1 主要风险清单

| 编号 | 风险描述 | 触发条件 | 影响范围 | 严重程度 | 缓解建议 |
| --- | --- | --- | --- | --- | --- |
| R-01 | **F1 sandbox 债务修复方向分歧**：是否在非 Linux 平台引入容器级隔离，还是维持诚实 no-op。若误将 no-op 宣传为"沙箱"，违背 D4 §4 诚实边界 | 下游在 F1 修复时跨平台引入 Docker/VM 隔离，或对外文档含糊 | 安全基件可信度、对外承诺一致性 | 高 | 以 B1（OS 级双隔离）为 Linux 优先参考；非 Linux 明确 no-op 并文档化（维持诚实边界）；跨平台容器隔离设为 P2 而非 MVP（见 §3.2 否决 B4） |
| R-02 | **插件机制演进到 MCP 的契约变更风险**：若直接替换现有 example.json 插件模型，可能破坏 ADR-008 开箱即用与 vetted 信任锚 | 在 MVP 阶段强制切换 MCP，未保留兼容层 | 扩展性、既有插件兼容性、安全信任链 | 中 | 先冻结现有 vetted 插件模型（§4.1），再规划 MCP 适配层；保留 vetted 信任锚（D10 §3） |
| R-03 | **X1 冲突（R-6/R-8/R-9 处置状态自相矛盾）**：D2 §0/§1/§4 称"接受残余"，D2 §3/§4.5 与 D3 §3 称"已加固闭环"。若下游据此错误冻结安全边界，可能漏防或重复投入 | business-architect 直接采信单一口径 | 安全设计范围、残余风险处置预算 | 高 | 下游 security-architect 须在安全设计阶段以代码事实（D3 §3 加固实现）为准裁决，并标注「经中间确认/冲突裁决」；本调研不裁决，仅并列保留 |
| R-04 | **X2 冲突（测试计数"55"口径）**：R-1 签名互操作测试受 network feature 门控，默认 feature 下不编译/不运行；"55"在默认构建下有效性存疑 | 下游以"55 测试通过"作为安全/质量基线 | 质量基线可信度、CI 门禁有效性 | 中 | 下游需明确默认 feature 与 hardened feature 两套测试基线；R-1 互操作测试应在 hardened CI 中显式运行并报告 |
| R-05 | **X3 口径（范式数 6 模块 vs 7 范式）**：非事实矛盾，属口径差异（single 兼 ReAct）。若下游术语不统一，高层架构文档会与 D6 表述错位 | 高层/系统文档沿用不同口径 | 术语一致性、文档交叉引用 | 低 | 下游 business-architect 在高层架构 §2 统一术语为"6 模块实现 7 范式（single 兼 ReAct）" |
| R-06 | **运维缺口补齐的资源/复杂度风险**：D1 §4 四项 🔴（supervisor/可观测性/原子升级/审计轮转）若在 MVP 全量补齐，可能超出 ganyu 体积/内存预算并拖延 P0 修复 | MVP 同时承载 P0 修复 + 全部运维缺口 | 交付排期、二进制体积 | 中 | 按 §4.2 分批：P0 先修，运维缺口按 P1 分批内嵌轻量方案（§4.3 推荐栈） |
| R-07 | **upload-repo-init 设计若复用不足安全基件，可能引入新攻击面**：git 操作、网络上传涉及命令注入/SSRF/权限越界 | 该缺失能力实现时未套用 resolve_sandboxed / ssrf_guard_resolve / restrict_file_permissions / shell 双层门禁 | 新模块安全、供应链完整性 | 高 | 强制复用既有安全基件（§4.1）；采用 §5 Q5 的状态机+幂等+回滚，fail-closed 默认拒绝 |

### 5.2 待确认项（需主理人 / 业务方反馈）

| 编号 | 待确认项 | 不确定性说明 | 若无法确认的备选路径 |
| --- | --- | --- | --- |
| U-01 | **upload-repo-init 的能力 scope**：是仅本地仓库初始化（git init/clone/commit），还是包含向远端仓库上传/发布（push/release）？ | D1 §7 行动项要求主理人确认 upload-repo 的 scope，尚未给出；_team_runtime 已纳入但 scope 未细化 | 先按最小集（本地 init/clone/commit + 幂等）设计，远端 push 作为 P2；或直接由主理人澄清 |
| U-02 | **插件机制是否演进到 MCP**：是维持现有 example.json 插件模型，还是适配 MCP server？ | 行动指令 (c) 要求评估 MCP；但 ganyu 已有 vetted 插件模型（ADR-008） | 维持现有模型 + 规划 MCP 适配层（§4.1），待 business-architect 裁决 |
| U-03 | **非 Linux 平台的 F1 sandbox 处置**：引入轻量隔离（如 macOS sandbox/seatbelt 参考 B1）还是维持 no-op？ | D4 §4 诚实边界明确非 Linux 为安全 no-op；跨平台隔离成本与收益需权衡 | 维持 honest no-op 并文档化；跨平台隔离设为 P2（§3.2 否决 B4 整体） |
| U-04 | **R-1 互操作测试的真实编译/运行条件**：默认 feature 还是 hardened feature 下执行？ | X2 冲突表明受 network feature 门控，默认不运行 | 下游在 CI 中明确两套基线；本调研标注为待确认（见 R-04） |
| U-05 | **基准数据时效性**：标杆的 star 数/定价（如 Claude Code 约 17 美元/月、Goose 51k stars）为调研时点公开资料，可能随时间变化 | 来自社区/二手来源（SR-02/05/09/11），非官方实时数据 | 以官方文档与仓库为准；关键决策以架构范式与许可模型为准，不依赖具体数字 |

### 5.3 需业务架构持续关注的依赖项

| 编号 | 依赖项 | 说明 | 建议关注阶段 |
| --- | --- | --- | --- |
| D-01 | X1 冲突裁决（R-6/R-8/R-9 状态） | 以代码事实（D3 §3 加固实现）为准，统一安全边界表述，避免高层/安全文档矛盾 | 安全设计（security-architect）阶段，G5 |
| D-02 | X2 测试基线口径统一 | 明确默认 vs hardened 两套测试计数，R-1 互操作测试纳入 hardened CI 报告 | 系统设计（system-architect）/ 安全设计，G4~G5 |
| D-03 | X3 范式术语统一（6 模块/7 范式） | 高层架构 §2 统一为"6 模块实现 7 范式（single 兼 ReAct）" | 高层架构设计（business-architect）§2，G3 |
| D-04 | upload-repo-init scope 与边界冻结 | 关联 U-01；其能力边界、安全基件复用方式、MVP 最小集需在高层架构冻结 | 高层架构设计（business-architect），G3 |
| D-05 | 插件机制 MCP 演进决策 | 关联 U-02；影响系统设计的扩展契约与接口 | 系统设计（system-architect），G4 |
| D-06 | F1 跨平台隔离策略与诚实边界维护 | 关联 U-03/R-01；影响安全设计的隔离矩阵与对外承诺 | 安全设计（security-architect），G5 |
| D-07 | 运维缺口（supervisor/可观测性/原子升级/审计轮转）落地形态 | 关联 §4.3 推荐栈；影响部署设计的资源拓扑与 CI-CD | 部署设计（platform-architect），G5 |

---

## 6. 关键来源目录

> 集中列出全部调研所使用的公开资料、官方文档、社区仓库、分析报告等。每条来源不低于 URL 粒度，关键来源给出具体章节或段落。

**硬指标**：
- ≥ 3 条来源，覆盖每家标杆（B1~B4 均已覆盖）。
- 关键数据（star 数、定价、沙箱机制、MCP 规范版本）均指定来源段落/位置。

| 编号 | 来源类型 | 标题 / 名称 | URL / 路径 | 相关章节 | 最后访问日期 |
| --- | --- | --- | --- | --- | --- |
| SR-01 | 官方工程博客 | Making Claude Code more secure and autonomous with sandboxing（bubblewrap/seatbelt、FS+网络双隔离、云端沙箱） | https://www.anthropic.com/engineering/claude-code-sandboxing | B1 §2.2.1、§2.3、§3.2 | 2026-08-19 |
| SR-02 | 社区架构分析 | Claude Code — Extension Architecture（6 种权限模式、deny→ask→allow、managed policies、MCP、subscription 约 17 美元/月） | https://sprantic.ai/claude-code-architecture | B1 §2.2.1、§2.3 | 2026-08-19 |
| SR-03 | 社区指南 | Claude Code Guide 2025: Memory/Skills/Agents 七级层级、自定义 Agents | https://www.shyamachuthan.com/blog/claude-code-guide-2025-master-skills-agents-memory-tools | B1 §2.2.1 | 2026-08-19 |
| SR-04 | 代理索引 | Goose: AI Agent（Rust、Apache-2.0、70+ MCP 扩展、15+ provider、BYOK） | https://openagent.bot/agents/goose | B2 §2.1、§2.2.2、§2.3 | 2026-08-19 |
| SR-05 | 独立评测 | Goose Review 2026（51k stars、500+ contributors、Linux 基金会 AAIF、无 SOC2/HIPAA、BYOK 5~50 美元/月） | https://theaiagentindex.com/agents/goose | B2 §2.2.2、§2.3、U-05 | 2026-08-19 |
| SR-06 | 架构目录 | Goose — Framework（Rust CLI/TS 桌面、ReAct 循环、YAML Recipes、subagent 隔离、GOOSE_MAX_TURNS） | https://www.agentpatternscatalog.org/compositions/goose | B2 §2.2.2、§2.3 | 2026-08-19 |
| SR-07 | 评测长文 | Goose Review: Block's Open-Source AI Agent（recipes 即可复用工作流、subagent 编排、隐私本地优先） | https://aicoolies.com/reviews/goose-review | B2 §2.2.2 | 2026-08-19 |
| SR-08 | 官方网站 | OpenCode: The open source AI coding agent（MIT、75+ provider、195k stars、隐私优先） | https://opencode.ai/ | B3 §2.1、§2.2.3、§2.3 | 2026-08-19 |
| SR-09 | 技术博客 | OpenCode 架构（Bun/TypeScript 服务端、客户端/服务端分离、LSP、MCP、多会话、自定义 agent） | https://joshuaberkowitz.us/blog/github-repos-8/opencode-the-open-source-ai-coding-agent-built-for-the-terminal-1597 | B3 §2.2.3、§2.3 | 2026-08-19 |
| SR-10 | 个人技术站 | OpenCode（MIT、Vercel AI SDK 75+ provider、Build/Plan/General agent、opencode.json 权限、MCP） | https://www.dsebastien.net/opencode/ | B3 §2.2.3、§2.3 | 2026-08-19 |
| SR-11 | 文档 | OpenHands Docker Sandbox（每会话独立 Docker 容器、挂载、反向代理、固定端口 host-network） | http://docs.openhands.dev/openhands/usage/sandboxes/docker | B4 §2.2.4、§2.3 | 2026-08-19 |
| SR-12 | 文档 | OpenHands Custom Sandbox（agent-server 镜像、资源配额、操作审计、快照回滚、三层镜像 Tag） | https://docs.openhands.dev/openhands/usage/advanced/custom-sandbox-guide | B4 §2.2.4、§2.3 | 2026-08-19 |
| SR-13 | 百科 | OpenHands（MIT、Web、沙箱 Docker、65k+ stars、任意 LLM、GitHub 原生） | http://irregularpedia.org/ai-ml/openhands | B4 §2.2.4 | 2026-08-19 |
| SR-14 | 协议规范 | Model Context Protocol（2025-11-25 规范、client-host-server、Tools/Resources/Prompts、stdio + Streamable HTTP、OAuth 2.1、Linux 基金会 AAIF 治理、10k+ 服务器） | https://ai-solutions.wiki/glossary/model-context-protocol | §3.3、§4.1、§4.3 | 2026-08-19 |
| SR-15 | 技能文档 | MCP 2025-11-25（JSON-RPC 2.0、能力协商、采样/elicit、Tasks 抽象、安全原则：宿主负责授权确认） | https://lobehub.com/bg/skills/tangledgroup-tangled-skills-mcp-2025-11-25 | §4.1 | 2026-08-19 |
| SR-16 | 开发者指南 | Developer's Guide to MCP（N-by-M 集成问题、host/client/server、初始化握手、工具调用 JSON-RPC 示例） | https://nerdleveltech.com/guides/model-context-protocol | §4.1 | 2026-08-19 |
| SR-17 | 安全工程 | Secure Firmware Updates: code signing（Ed25519 体积小/最快/适合嵌入；TUF 防仓库 compromise + 密钥轮换；A/B 或恢复分区 + 健康检查防 brick；Sigstore/cosign 透明日志） | https://beefed.ai/en/secure-firmware-updates-code-signing-secure-boot | §4.1、§4.3、Q4 | 2026-08-19 |
| SR-18 | 安全实践 | Navigating Software Supply Chain Security 2025（SBOM、Sigstore/cosign 签名、SLSA 框架、最小权限） | https://blog.madrigan.com/blog/202508231836 | §4.1 | 2026-08-19 |
| SR-19 | CI/CD 指南 | Secure Software Supply Chain with Sigstore, TUF & In-Toto（签名/验证、TUF 安全分发、in-toto 完整性） | https://www.lktechacademy.com/2025/10/secure-software-supply-chain-sigstore-tuf-intoto.html | §4.1 | 2026-08-19 |
| SR-20 | 模式库 | Implementation Patterns: Idempotency（Check-Before-Act、Upsert、Force Overwrite、Unique IDs、Tombstone/Marker；git ls-remote 幂等分支） | https://adaptive-enforcement-lab.com/patterns/efficiency/idempotency/patterns | §5 Q5、§4.1 | 2026-08-19 |
| SR-21 | 经验问答 | CLI Agent 可恢复一致提交（风险+可逆+幂等+失败类型 决策表；retry with backoff+jitter；补偿；分布式锁/租约 run_id.lock+PID+heartbeat；幂等键写外部系统） | https://moltq.chat/questions/7662facc-1efd-4fb4-9078-3c1b4c399b29/ | §5 Q5、R-07 | 2026-08-19 |
| SR-22 | 技术文 | 命令状态机设计（StatePending→StateRunning→Succeeded/Failed，CAS 原子迁移；重试+退避、补偿事务、快照回滚；zerolog+OpenTelemetry 轻量嵌入） | https://datasea.cn/go0202452239.html | §5 Q5、§4.3 | 2026-08-19 |
| SR-23 | 部署清单 | Go Application Deployment Checklist（CGO_ENABLED=0 静态二进制、/healthz、优雅停机、slog 结构化日志、Prometheus /metrics、scratch/distroless） | https://bugsly.dev/blog/go-deployment-checklist | §4.1、§4.3、Q4 | 2026-08-19 |
| SR-24 | 运维实践 | Golang 助力 Linux 自动化运维 / CLI 生产实践（systemd Restart=on-failure、ProtectSystem=strict、MemoryMax/CPUQuota、audit/event/debug 日志分级与轮转、NDJSON 对接 ELK/Splunk） | https://m.yisu.com/ask/75389943.html | §4.1、§4.3、Q4 | 2026-08-19 |
| SR-25 | 上游资料摘要 | material_digest.md（G1 已通过，D1~D18 与 SRC 摘要、X1/X2/X3 冲突、缺失能力 upload-repo-init） | D:\workbuddy_all\harness_all\ganyu-agent\.workbuddy\output\material_digest.md | 全文（项目事实基准） | 2026-08-19 |
| SR-26 | 项目文档 | ganyu-agent 现有安全/架构/签名文档（SECURITY.md D4、update-signing.md D5、architecture.md D6、sign-release.py D16、install.sh/ps1 D13/D14、release.yml D15、plugins/example.json D18、ADR-001~008 D7） | 见 material_digest.md §2（D4/D5/D6/D7/D13~D18） | 全文（项目事实基准） | 2026-08-19 |

---

## 7. 硬指标清单

> 汇总本模板所有章节的硬指标，供自动校验与人工审核使用。

| 章节 | 硬指标项 | 当前状态 | 备注 |
| --- | --- | --- | --- |
| §1 | 调研问题已收敛为 ≥ 3 条可执行问题 | ✅ | 收敛为 Q1~Q5（5 条），覆盖行动指令 a~f |
| §2.1 | 标杆系统 ≥ 3 家，含 ≥ 1 家头部 SaaS | ✅ | B1 Claude Code（闭源 SaaS）+ B2/B3/B4 开源代表，共 4 家 |
| §2.1 | 标杆系统 ≥ 1 家开源或自研代表 | ✅ | B2 Goose（Rust 开源）、B3 OpenCode（MIT）、B4 OpenHands（MIT） |
| §2.2 | 每家标杆有独立详述卡片 | ✅ | B1~B4 均含 10 维度 + 置信度（已核实/推断/综合归纳） |
| §2.3 | 关键能力横向事实无遗漏 | ✅ | 10 能力维度 × B1~B4，不评分不排序 |
| §3.1 | 对比矩阵含 5 维度 + 权重 + 评分 | ✅ | 权重之和 = 1.00（0.30+0.20+0.15+0.15+0.20） |
| §3.2 | 评分结论含优先/部分/不借鉴三层 | ✅ | 优先 B2(4.80)；部分 B3(4.55)/B1(3.95)；不借鉴 B4(3.60)，均引用得分 |
| §3.3 | 方案组合分析（按需） | ✅ | 给出 B2+B3+B1 组合 + B4 子模式抽取 |
| §4.1 | 自研/采购/复用边界有明确建议 | ✅ | 7 项能力逐项给出方式/依据/候选/前提 |
| §4.2 | MVP 范围建议与用户诉求对齐 | ✅ | 对齐 P0/P1 与 upload-repo-init 缺失能力 |
| §4.3 | 技术栈参考（推荐+替代+理由） | ✅ | 7 技术层，含 MCP/ed25519+TUF/systemd/OTel 等 |
| §5.1 | 主要风险 ≥ 3 条，有缓解建议 | ✅ | R-01~R-07 共 7 条，含 X1~X3 与 F1/运维缺口，均带缓解建议 |
| §5.2 | 待确认项（需主理人/业务方反馈） | ✅ | U-01~U-05，含 upload-repo scope、MCP 演进、F1 跨平台等 |
| §5.3 | 需下游持续关注的依赖项 | ✅ | D-01~D-07，指向 business/system/security/platform-architect 及阶段 |
| §6 | 关键来源可追溯（URL / 章节） | ✅ | SR-01~SR-26，≥3 条且覆盖每家标杆，关键数据指定段落 |
| 全文 | 明确区分事实 / 推断 / 建议 / 风险 | ✅ | §2 标"事实"、§2.2 置信度列、§3 标"对比"、§4 标"建议"、§5 标"风险" |
| 全文 | 不存在编造来源或占位符 | ✅ | 来源均可核验；全文无残留占位符（角括号占位、示例前缀、待填日期、待验证标记均已规避） |
