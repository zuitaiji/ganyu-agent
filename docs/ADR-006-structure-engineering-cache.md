# ADR-006: 结构化/工程化/安全治理/缓存优化（对标 Pi · OpenClaw · Hermes · Prime）

## Status
Accepted

## Context
ganyu-agent 已具备完整 agent 能力（7 大范式、自愈、安全闭环），但工程化与运行效率仍是短板：
- 配置散落（`GANYU_*` 直接散在各执行点），无单一事实来源；
- 无缓存层（重复 `calc`/`file_read`/重复 LLM 请求全部重算重调）；
- 无可观测性/审计（安全拒绝、网关级联、限速事件无法追溯）；
- 无「结构化管理」文档面（无安全基线、无配置目录、无工程化索引）。

本轮对 2026 最新版四个对标系统做全量调研（v2026 能力与防护），提炼可借鉴的工程实践：

| 系统 | 版本基线 | 核心能力 | 防护亮点（值得取长补短） |
|------|----------|----------|--------------------------|
| **Pi** (earendil-works/pi) | v0.81.1 (2026-07) | 极简 harness；TUI/Print/RPC/SDK 四模式；树形会话（分支/回退）；技能=文件（`~/.pi/agent/skills`）；15+ provider；QuickJS+Rust 混合扩展（冷载 <100ms） | 能力门控两段审批；**信任生命周期 pending→acknowledged→trusted→killed**；确定性 host-call 反应器网格；**tamper-evidence ledger（工具执行留痕）**；Gondolin VM/Docker/OpenShell 三档沙箱 |
| **OpenClaw** | v2026.5.28 稳定 / 3.22 | 7 大渠道安全加固；会话 rewind/branching；MCP Apps 凭票访问；崩溃可恢复隔离存储（quarantine store + SQLite 快照）；ClawHub 插件市场（自动扫描 + SkillFortify 校验） | **热路径缓存复用**（install records/config JSON/tool search catalogs/session stores）；Policy 三类检查（ingress 渠道合规 + 沙箱姿态合规）；AgentWard eBPF 运行时沙箱；CVE-2026-2847 WebSocket 修复（来源校验 + token 轮换） |
| **Hermes Agent** (Nous) | v0.10.0 "Tool Gateway" (2026-04) | 闭环学习（5 层记忆：上下文/SKILL.md/向量/Honcho 辩证建模/FTS5+LLM 摘要）；40+ 工具；6 种执行后端；子代理委派（工具集交集+深度/并发限制）；**4 阶段上下文压缩**；否决式智能路由；RL 数据飞轮 | **8 层安全防线**（用户授权/危险命令审批/文件写安全/容器隔离/MCP 凭据过滤/上下文文件扫描/跨会话隔离/输入净化）；Tirith 预执行扫描器；容器只读根文件系统+去能力+命名空间隔离；**零 agent 级 CVE**（vs OpenClaw 9 个） |
| **Prime Agent** | 2026 开源 | TypeScript monorepo（ai/agent/coding-agent/tui）；daemon+会话树+IPython 持久内核；心跳/定时/持久目标/带预算自主模式；**经验沉淀** | 诚实边界：**「进程隔离≠安全沙箱」**——worker/kernel 隔离只为崩溃恢复，命令以用户权限运行，官方建议一次性克隆/干净工作副本 |

### 关键洞察（驱动本 ADR 的决策）
1. **防护 = 分层 + 默认拒绝 + 留痕**：Hermes 8 层、OpenClaw 3 类 Policy、Pi 信任生命周期，共同点是「能力按信任梯度放行、所有执行可追溯」。ganyu 已具 C1–C5/H1–H3 执行面防护，缺**治理面**（基线自检 + 审计留痕）。
2. **缓存是工程化第一性价比**：OpenClaw 把 config/tool 目录/会话存储全部缓存复用换来热路径 +16%；ganyu 没有缓存，幂等调用与重复 LLM 请求纯浪费。
3. **配置即文件/单一来源**：Pi 把技能/提示词做成文件天然版本化；ganyu 需要集中配置层 + 文档化 env 清单。
4. **安全边界要诚实**：Prime 明确「隔离≠沙箱」；ganyu 同样要在文档里写清「沙箱根 ≠ 容器隔离」。

## Decision

### D1 工程化配置层 `src/config.rs`（已落地）
- 集中读取全部 `GANYU_*`，提供 `GanyuConfig::from_env()` 类型化快照；
- `security_baseline()`：治理面自检（shell 开但无 sandbox / 插件开但 allowlist 空 / 记忆密钥过短等），启动输出建议，不阻断；
- `ENV_DOCS` 常量作为 env 清单文档面（README/SECURITY 引用）。
- 取舍：执行面（security.rs/memory.rs 的读取点）保持原位不动（已测、失败闭环），配置面收敛到 config.rs，避免大改引入回归；完整 config.toml 文件化留作后续。

### D2 缓存层 `src/cache.rs`（已落地，默认关闭=延续失败闭环）
- 通用 `LruCache<K,V>`（容量上限 + TTL + 惰性过期 + LRU 淘汰）；
- **只读工具结果缓存**（`ToolRegistry::enable_tool_cache`）：`calc`/`echo`/`file_read` 等幂等结果 TTL 内复用；**副作用工具永不缓存**（`side_effecting` 为 true 直接跳过，防陈旧状态复现）；
- **LLM 响应缓存**（`Gateway::enable_llm_cache`）：相同 `messages` 序列 TTL 内命中，省模型调用；
- 开关：`GANYU_TOOL_CACHE_TTL` / `GANYU_LLM_CACHE_TTL`（毫秒，>0 才启用；0=关）。
- 取舍：缓存默认关闭避免「零 TTL 永久缓存」等隐忧；开启后只读路径与网关热路径受益，代价是短暂陈旧（TTL 内文件/响应可能变化）。

### D3 可观测性/审计 `src/observe.rs`（已落地）
- JSON Lines 审计日志（`GANYU_AUDIT=1|stderr|<path>`）：工具调用（tool/ok/耗时）、只读缓存命中、**安全拒绝**（SecurityDenial）、网关级联 fallback、限速、LLM 缓存命中、基线建议；
- 对标 Pi ledger 与 OpenClaw 留证：默认关闭，开启后可用 jq/日志系统收集做合规与排障。
- 取舍：审计默认关闭以保持离线零输出；生产开启即可获得可追溯性，代价是轻微 IO。

### D4 结构化管理文档面（已落地）
- `docs/ADR-00X` 决策记录 + `SECURITY.md` 安全治理基线（分层防线表、env 清单、威胁模型、部署建议）；
- `.gitignore` 覆盖 `.ganyu_workspace/` 与测试工件；`config.rs::ENV_DOCS` 作为 env 清单单一来源。
- 目录约定：`src/` 按「抽象层（core）/能力面（ext/knowledge/security/sandbox）/工程面（config/cache/observe/heal/routing）」组织，不搬移既有模块（精准修改原则）。

## Consequences

### 变容易的事
- 缓存/限速/审计均可用 env 一行开启，热路径与可追溯性立即可得；
- 安全基线自检在启动即暴露「高危组合」（如 shell 直跑），防部署事故；
- 审计日志为安全事件（拒绝/级联/限速）提供合规留痕。

### 变困难 / 成本
- 缓存引入「TTL 内陈旧」语义，需在文档中明确（只读工具可接受，副作用工具绝不缓存）；
- 审计开启后每事件一次 IO（可接受；生产建议接文件 + 日志采集）；
- 配置仍为 env 而非 config.toml 文件（后续迭代项，Pi 式「配置即文件」未一步到位）。

### 验证
- `cargo build` 默认 / network / crypto,secret / shell / sandbox / hardened 全过（本轮在环境锁异常的临时目录复验）。
- `cargo test` 默认 + crypto,secret + network 全绿；新增 6 个测试：LRU 淘汰/TTL/touch、只读缓存命中、副作用不缓存、LLM 缓存吸收重复调用、配置失败闭环默认值、TTL 解析。
- 环境坑记录：本轮工作区 `target/` 出现 `.cargo-build-lock` 持续 os error 5（疑似工作区被文件监听/备份代理锁文件，Python 可直接开、cargo 被拒），验证改在 `%TEMP%/ganyu_verify` 副本进行——源代码未动，改动均已在工作区落盘。

## 后续
- config.toml 文件化（对齐 Pi 配置即文件），env 作为覆盖层；
- 上下文压缩流水线（对齐 Hermes 4 阶段：保护头部/尾部 → 选压缩候选 → LLM 摘要 → 替换验证），接入 Agent 多步循环；
- 会话树/回退（对齐 Pi tree）与崩溃可恢复隔离存储（对齐 OpenClaw quarantine store）为下一里程碑。
