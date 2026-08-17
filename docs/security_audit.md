# ganyu-agent 安全审查 + 流程审查报告

> 范围：对 `src/` 全部安全敏感代码与所有工作流（single/react/plan/multi/router/blackboard/graph + 技能派发）做全量人工审查。
> 方法：源码逐文件审查（security.rs、ext/builtins.rs、ext/mod.rs、ext/skills.rs、ext/nomifun_caps.rs、config.rs、core/llm.rs、routing/mod.rs、core/memory.rs、core/loop_.rs、core/workflow/*、main.rs）+ `cargo test --release` 验证（36 单元 + 5 集成 + 8 工作流全绿）。
> 结论：架构的安全基线**整体扎实**（FS 沙箱、SSRF 防护、无 shell 的插件执行、密钥擦除、边界清洗均有），但存在若干**生产部署前必须处理的缺陷**，集中在「密钥/记忆存储明文、自更新完整性、计划步数无上限、提示注入传播、技能参数注入」。

---

## 0. 总体结论

| 维度 | 评价 |
|------|------|
| 文件系统沙箱 | ✅ 强（`security::resolve_sandboxed` 拒绝绝对路径/`..`/NUL + 前缀校验） |
| 网络 SSRF | ✅ 强（`ssrf_guard` 拒绝私有/环回/元数据/fake-ip 感知，禁重定向并重检 Location） |
| 命令执行模型 | ✅ 强（插件 `CommandTool` 直接 spawn、无 shell、多层 fail-closed 门禁） |
| 密钥处理 | ⚠️ 默认构建不擦除、配置文件明文存储 |
| 自更新 | ⚠️ 缺校验时静默放行 + tar 解包无穿越防护 |
| 流程边界 | ⚠️ 计划步数无上限；多 agent/黑板跨 agent 注入传播 |

**严重度统计**：Critical 0 · High 1 · Medium 6 · Low 6。

---

## 1. 详细发现

| ID | 严重度 | 位置 | 问题 | 建议 |
|----|--------|------|------|------|
| F-01 | Medium | `config.rs` `write_model_config` / `setup` | API Key 以**明文**写入 TOML（`~/.ganyu/config.toml` 或 `./ganyu.toml`）。显示侧已 masked，但落盘明文。 | 写入时 `chmod 600`；优先从环境变量读取；生产建议结合 OS keychain。 |
| F-02 | Medium | `main.rs`（`.ganyu_memory.json` 默认 CWD）/ `memory.rs` | 记忆文件默认落在**当前工作目录**且默认**明文**（需 `crypto` + `GANYU_MEM_KEY` 才加密）。可能含敏感对话/凭据，易被提交进仓库。 | 默认路径改到 `~/.ganyu/`；生成 `.gitignore` 规则；生产强制 `GANYU_MEM_KEY`。 |
| F-03 | High | `main.rs` `update`（≈L520-560） | 自更新校验：当 `.sha256` 资源缺失时分支**直接跳过校验**并继续解包安装。 | **失败闭环**：校验资源不可用时拒绝更新（除非显式 `--no-verify`）。 |
| F-04 | Medium | `main.rs` `update`（`tar -xzf tmp -C bin_dir`） | tar 解包未校验条目路径，恶意 Release 可用 `../` 写出沙箱目录外（覆盖任意文件）。 | 用 Rust `tar` crate 逐条检查 `Entry::path()` 不超出 `bin_dir`；或仅允许单个预期二进制条目。 |
| F-05 | Medium | `core/workflow/plan_execute.rs` | 计划步数来自 planner（LLM）输出按行拆分，**无步数上限** → 资源耗尽 / 失控执行。 | 设 `MAX_STEPS`（如 20），超出截断或报错。 |
| F-06 | Medium | `core/workflow/multi_agent.rs` / `blackboard.rs` / `loop_.rs` | **间接提示注入传播**：工具返回（`web_fetch`/文件/同步的 `SKILL.md` 等不可信内容）进入 prompt，并在多 agent/黑板中跨 agent 累积传递，可逐步劫持后续 agent。 | 将工具输出视为「不可信数据」；在系统层标记指令/数据边界；对不可信输入启用 observer/只读模式，避免其驱动高权限工具。 |
| F-07 | Medium | `src/ext/nomifun_caps.rs` `NomifunSkillTool::invoke` | 参数编码 `cap=<name> {input}`：若用户输入（或多步流程中上游工具输出）含 `cap=`，可**重定向到另一个能力**（能力混淆）。 | 把能力名编码到用户不可覆盖的位置：例如 `cap=<name>\n{input}`，按**首个换行**切分取能力名，而非按空格。 |
| F-08 | Low | `nomifun_caps.rs` `is_safe_gateway_prog` | 允许 `sh/cmd/powershell/bash` 作为网关程序；若运维误设 `GANYU_NOMIFUN_GATEWAY='sh -c "..."'`，`{input}` 会落入 shell 字符串（实测因 `split_whitespace` 破坏引号而基本失效，但仍应硬化）。 | 网关程序拒绝常见 shell 解释器；并校验模板不含 shell 元字符。 |
| F-09 | Low | `Cargo.toml` `default = []` | 默认构建**不含** `secret`(密钥不擦除)/`crypto`(记忆明文)/`sandbox`/`network`。`hardened` 组合也**未含 `sandbox`**（Landlock 仅 Linux）。 | 生产必须 `--features hardened`（Linux 再加 `sandbox`）；考虑 release profile 默认开启 `secret`/`crypto`；`security_baseline()` 已告警，可升级为强约束。 |
| F-10 | Low | `config.rs` `load_model_config` | 读取配置后 `std::env::set_var("OPENAI_API_KEY", k)` 写入进程全局环境，任何环境变量转储（崩溃/诊断）都可能泄露。 | 用结构体在内存传递密钥，避免全局 `set_var`。 |
| F-11 | Low | `security.rs` `resolve_sandboxed` | 校验在**未最终 canonicalize** 的路径上做 `starts_with`：若沙箱内存在名为 `name` 的**符号链接**指向外部，前缀检查通过但写入会穿透。 | 对最终路径再 canonicalize 并重检；或打开时 `O_NOFOLLOW`/Win32 禁止最后分量跟随符号链接。 |
| F-12 | Low | `main.rs` `sha256_of_file` | 路径非 UTF-8 时 `unwrap_or_default()` 把空串传给 `certutil`/`sha256sum`。 | 非 UTF-8 直接返回错误，而非空路径。 |
| F-13 | Info | `ext/builtins.rs` `web_fetch` | 仅在 `network` 特性编译；SSRF 防护良好，但 DNS 重绑定属客户端固有风险（已文档化）。 | 关键场景可在出网网关侧加 DNS 钉选/解析后二次校验。 |
| F-14 | Low | `core/workflow/router.rs` | 关键字路由 first-match-wins + 子串命中（与 nomifun 路由同类问题）；属运维自有配置，影响较小。 | 同 nomifun：最长关键词优先。 |

### 亮点（值得保留）
- FS 沙箱拒绝绝对/`..`/NUL + 前缀校验；SSRF 拒绝私有/环回/元数据 + 禁重定向重检。
- 插件 `CommandTool` 直接 spawn（**无 shell**），门禁链：`GANYU_ALLOW_PLUGINS` → `vetted:true` → `GANYU_PLUGIN_ALLOW` 白名单 → `is_safe_program`，且调用时**二次校验**（fail-closed）。
- ExecTool（`sh -c`/`cmd /c`）仅在 `shell` 特性编译 **且** `GANYU_ALLOW_SHELL=1` 时启用——默认双关，高风险能力默认不可达。
- 网络后端：HTTPS、未禁用 TLS 校验、`base_url` 来自运维配置（非用户输入，限制 SSRF）、密钥在 `secret` 特性下用 `zeroize::Zeroizing` 擦除、错误不回显密钥。
- `model`/`setup`/`doctor` 对密钥做 masked 显示，不泄露明文。
- `sanitize_model_output` 在信任边界清洗 NUL/控制字符并截断 1M。
- 图工作流 Kahn 拓扑 + 环检测；其余工作流以 `max_rounds` 有界。

---

## 2. 流程（工作流）审查

| 流程 | 实现 | 安全/正确性结论 |
|------|------|------------------|
| single / react | `AgentCore` + `run_react`（ReAct 循环，受 max-iters 护栏） | 工具调用经 `known` 集合校验，只能调用已注册且门禁通过的工具，**不能任意执行**。✅ |
| plan_execute | planner → 按行拆步骤 → executor 逐步执行 | ⚠️ **步骤数无上限**（F-05）；每步仍受 ReAct max-iters 约束。 |
| multi_agent | `max_rounds × units`，逐轮透传 transcript | 有界；⚠️ transcript 无上限累积（F-06 注入传播）；最终结果返回整段轨迹可能过大。 |
| blackboard | 共享黑板快照，多轮后合成器汇总 | 有界；⚠️ 共享状态使外部不可信内容对所有 agent 可见（F-06）。 |
| router | 关键字 → Unit，命中即执行，miss 走兜底 | first-match 子串（F-14，影响小）；有兜底不panic。 |
| graph | Kahn 拓扑排序，拒绝环；逐节点执行 + 多前驱拼接 | ✅ 不会死循环；`self.nodes.get(id).unwrap()` 的输入由拓扑保证存在，安全。 |
| 技能派发（`SkillTool`/`skills.rs`） | `SkillStep::Call` 的 `arg` 经 `{input}` 替换后调用工具 | ⚠️ 对 nomifun 技能存在 `cap=` 参数注入（F-07）；其余工具自身沙箱/门禁兜底。 |
| nomifun 派发（`nomifun_skill`） | 离线返回真实 `SKILL.md`；可选 `GANYU_NOMIFUN_GATEWAY` 真实桥接 | 离线内容来自受信源（nomifun 内置技能）；网关分支 `is_safe_gateway_prog` 约束程序名（F-08）。 |

**跨流程共性问题**：所有流程共用同一工具注册表与沙箱/门禁边界，因此「工具层安全」对全流程统一生效；唯一的系统性风险是 **F-06 提示注入**——它不破坏沙箱，但可在「允许联网/允许 shell」的配置下，借不可信外部内容逐步引导 agent 调用高权限工具。这是 agent 类系统的固有难题，需在「指令 vs 数据」边界上做持续加固，而非单点修复。

---

## 3. 修复优先级

- **P0（生产前置）**：F-03（自更新失败闭环）、F-04（tar 穿越）、F-05（计划步数上限）、F-07（`cap=` 注入）、构建 `--features hardened`(+`sandbox`)。
- **P1**：F-01/F-02（密钥与记忆存储）、F-06（注入边界）、F-11（符号链接）。
- **P2**：F-08/F-09/F-10/F-12/F-14。

---

## 4. 验证状态
- `cargo test --release`：36 单元（含 `catalog_has_no_duplicate_names`/`all_caps_registered_as_skills`/`every_cap_routes_by_keyword`/`synced_skill_files_present`）+ 5 集成 + 8 工作流，全部通过（RC=0）。
- 端到端：`ganyu tools` 列出 33 个 `skill:<name>`；`ganyu skill clean-code` 返回真实 `SKILL.md` 正文。
- 本次审查为**静态源码审查**，未改动代码；如需我直接落地 P0/P1 修复，请确认。

---

## 5. 第二阶段：STRIDE 模型与残余风险登记（R-1 ~ R-9）

> 在 F-01~F-14 全部闭环后，针对新增/残余风险做 STRIDE 建模与第二阶段加固。
> 详细报告见 `docs/SECURITY-REPORT.md`；加固实现见 `docs/security_fixes.md`「第二阶段加固」。

### 5.1 STRIDE 结论

| STRIDE | 现有控制 | 残余风险 | 评级 |
|--------|----------|----------|------|
| Spoofing | API Key / Telegram token 认证；本地单用户无远程身份面 | 无跨用户身份冒用 | 低 |
| Tampering | 记忆 AES-256-GCM（R-2）；config 0600(unix)；更新 sha256 **+ ed25519 签名**（R-1）；FS 沙箱 | Windows config 权限依赖 ACL（R-8/R-9）；OV 明文传输（R-3） | 低 |
| Repudiation | `observe::AuditLog`（无密钥回显） | 审计日志未防篡改存储 | 低 |
| Info Disclosure | 记忆加密（R-2）；`Zeroizing` 擦除；F-10 移除全局 env；masked 显示；SSRF；模型输出净化 | `GANYU_MEM_KEY` 仍存进程 env（R-4）；OV 明文（R-3） | 低 |
| DoS | 计划步数上限 20；`max_rounds`；模型输出 1M 截断；限速 | 黑板快照无硬上限（R-6） | 低 |
| EoP | Landlock（Linux）；shell 双层关；插件 allowlist；`cap=` 首行；SSRF；FS 沙箱 | tar 穿越已补（R-5） | 低 |

### 5.2 R-1 ~ R-9 处置

| ID | 风险 | 严重度 | 处置 |
|----|------|--------|------|
| R-1 | 自更新仅同源 sha256，发布方被接管无法鉴别 | Medium | **已加固**（ed25519 签名，`main.rs`，ring 0.17） |
| R-2 | 记忆密钥无盐/单次哈希，弱口令易暴破 | Medium | **已加固**（每文件盐 + 100k 轮 KDF，`ENC2:`，兼容 `ENC1:`） |
| R-3 | OpenViking `OV_BASE` 可能 http 明文外发 | Low | 接受（默认本地 `:1933`；建议 TLS/内网隔离） |
| R-4 | `GANYU_MEM_KEY` 存进程 env，崩溃/子进程可泄漏 | Low | 接受（已由 F-10 收敛；建议 OS 密钥环/KMS） |
| R-5 | tar 解包漏 Windows 盘符绝对路径 | Medium | **已加固**（`is_safe_archive_entry` 拒绝盘符绝对路径） |
| R-6 | 黑板快照无硬上限 | Low | 接受（`max_rounds` 间接有界；建议加字节上限） |
| R-7 | 死代码 `load_model_config` 含全局 `set_var` | Low | **已清理**（删除函数） |
| R-8 | `write_model_config` 仅 unix 0600，Windows 未收紧 | Info | 接受（建议 `icacls` 限属主） |
| R-9 | Windows 记忆/配置目录未做等价权限隔离 | Info | 接受（平台权限模型差异） |

### 5.3 第二阶段结论

F-01~F-14 全部闭环；本阶段按范围 **B（A + R-1）** 完成 HARD-1（记忆加密强化）、HARD-2（tar 盘符穿越）、HARD-3（死代码清理）、R-1（ed25519 签名）。R-1 把供应链完整性从"传输层"提升到"发布方身份层"，是本期最关键增益。R-3/R-4/R-6/R-8/R-9 均为 Low/Info，已记录并接受，按路线图后续消除。**生产部署仍强制 `--features hardened`（Linux 加 `sandbox`），并配置 `GANYU_MEM_KEY` 与 `GANYU_UPDATE_PUBKEY`。**

