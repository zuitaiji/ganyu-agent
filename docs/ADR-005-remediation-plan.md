# ADR-005: 缺陷与漏洞修复落地方案（P0–P3）

## Status
Accepted

## Context
ADR-003 审计出 5 个 Critical / 3 个 High / 6 个 Medium / 2 个 Low 缺陷，PoC 已确认 C1（exec RCE）
与 C4（任意文件读）在默认构建下可直接利用。ADR-004 对比了 2026 年主流开源 agent 的安全边界，
明确「默认拒绝、显式开启、可证明隔离」是它们的共同基线。本 ADR 记录把 P0–P3 落地为代码的具体
决策与取舍，目标是：**默认构建零利用面，任意能力开启都必须显式 opt-in 且最小权限**。

核心设计约束（来自 ganyu 自身定位）：
- **离线优先**：默认构建不引入 TLS/C/沙箱依赖，保证 `cargo build` 零网络、零系统库。
- **失败闭环（fail-closed）**：任何不确定输入一律拒绝，而非放行。
- **可拓展不打折**：修复不能删特性，只能加护栏。

## Decision

### 威胁模型与默认姿态
默认构建（无 feature）视为「不可信输入环境」：exec 不编译进二进制、插件不扫描、
文件 IO 锁死在 `.ganyu_workspace` 沙箱根、web_fetch 仅在 network 特性下存在且带 SSRF 防护。
任何「更强能力」都通过 feature / 环境变量双层开关开启。

### P0 — Critical（已落地，默认闭环）

| 编号 | 问题 | 决策 | 取舍 |
|------|------|------|------|
| C1 | `exec` RCE | `exec` 仅 `#[cfg(feature="shell")]` 编译；运行时再需 `GANYU_ALLOW_SHELL=1`。双层失败闭环。 | 默认失去本地执行能力；换取 RCE 面归零。需要本地执行时显式开启。 |
| C2 | 插件任意命令 | `discover` 默认返回 0（需 `GANYU_ALLOW_PLUGINS=1`）；清单项须 `vetted:true`；程序名须在 `GANYU_PLUGIN_ALLOW` 清单且通过 `is_safe_program`（禁绝对路径/穿越/元字符）。 | 默认插件系统不可用；换取「无需重编译扩展」不被滥用为 RCE。 |
| C3/C4 | 文件任意读/写 | 新增 `security::resolve_sandboxed`：拒绝绝对路径、`..` 穿越、NUL；`canonicalize` 后校验仍在沙箱根内（防符号链接逃逸）。默认根 `.ganyu_workspace`。 | 文件工具被关进沙箱；换取任意路径读/写归零。需访问系统文件时显式设 `GANYU_FS_ROOT`。 |
| C5 | `web_fetch` SSRF | `ssrf_guard` 拒绝非 http/https、含 `@`、本机/内网/链路本地/云元数据域名与私有 IP（含 169.254.169.254）；关闭自动重定向并对重定向目标二次校验。 | 出站更慢（需 DNS 解析）；换取内网探测/元数据窃取归零。DNS 重绑定无法纯客户端根除 → 强隔离应在出口代理。 |

### P1 — High（已落地）

| 编号 | 问题 | 决策 | 取舍 |
|------|------|------|------|
| H1 | 记忆明文落盘 | `crypto` 特性下 `LocalMemory` 用 AES-256-GCM 加密落盘；密钥由 `GANYU_MEM_KEY` 经 SHA-256 派生；缺密钥回退明文（仅警告）。原子写（tmp+rename）。 | 默认无加密（保持零依赖）；生产用 `--features crypto` 并设强 passphrase。生产应改从 OS 密钥环/KMS 注入原始 32 字节密钥。 |
| H2 | SQL 注入无防护 | `Mdl::detect_injection` 检测堆叠语句、注释截断、危险 DML/DDL/系统函数（词边界匹配）；`validate_sql` 计入问题使校验失败。`template_sql` 用白名单区域值 + 数值 `top_n` 构造，天然规避拼接注入。 | 生成侧兜底；真正强隔离仍应在执行层用参数化查询（Prepared Statement）。 |
| H3 | 无进程沙箱 | 新增 `sandbox` 特性：在 Linux 上对 `exec` 派生子进程经 Landlock 限制文件系统访问（沙箱根可写，系统库只读执行）。作为「目标特定依赖」仅在 Linux 拉取，避免破坏跨平台编译。非 Linux 为安全无操作。 | 跨平台主隔离仍是 C3/C4 文件沙箱；Landlock 是 Linux 上 exec 的加固层。完整隔离（syscall/网络/内存）应叠加 Docker/gVisor。 |

### P2 — Medium（已落地）

| 编号 | 问题 | 决策 |
|------|------|------|
| M1 | OpenViking 空实现 | `network` 特性下 `OpenVikingMemory` 真正代理 `OV_BASE` 的 `:1933` REST（put/get/search），任何网络错误自动降级本地安全网。无 `OV_BASE` 时等同本地。 |
| M2 | 无限速 | `Gateway::with_rate_limit(per_min)` 令牌桶；`complete` 前扣减，超限返回 `RateLimited`（自愈分流）。默认不限速（显式开启）。 |
| M3 | 副作用工具盲目重试 | `Tool::side_effecting()` 默认 false；`ToolRegistry::call` 对副作用工具（file_write/exec/remember/command）失败即失败，不重试，避免重复写/发/执行。 |
| M4 | 阻塞 IO + 竞态 | `LocalMemory.store` 改 `tokio::sync::Mutex` + 异步 `save`，原子写；并发写测试 50 路无丢失。 |
| M5 | 无模型输出校验 | `security::sanitize_model_output` 在 `Gateway::complete` 出口处执行：去 NUL、去控制字符、超 1MB 拒绝，防输出洪泛/注入。 |
| M6 | 无原生函数调用 | `loop_::parse_tool_call` 同时支持 JSON 原生函数调用（`{"tool","args"}` / OpenAI `function_call` 风格）与向后兼容的 `@tool arg` 脚本语法。 |

### P3 — Low（已落地）

| 编号 | 问题 | 决策 |
|------|------|------|
| L1 | API key 明文 `String` | `secret` 特性下 `OpenAiBackend.api_key` 改为 `zeroize::Zeroizing<String>`，Drop 时清零。默认仍为 `String`（保持零依赖）。 |
| L2 | 网关不可热更新 | `Gateway` 后端表改 `Mutex<Vec>`（register 改 `&self`），新增 `hot_reload(path)`（network 特性）从 JSON 配置原子替换后端并重置熔断器/粘路径。 |

## Consequences

### 变容易的事
- 默认构建的攻击面从「5 个可直接利用」降为 0；PoC（exec / file_read）复跑确认失败闭环。
- 安全能力可组合：`--features crypto,secret` 加固密钥与记忆；`--features hardened` 一键生产加固（不含 Linux-only 的 sandbox）。
- 新增的 `security` 模块与 `Tool::side_effecting`/`schema` 是后续能力的统一护栏点。

### 变困难 / 成本
- 默认失去本地执行与插件扩展（需显式开关）——这是有意为之的安全代价。
- 文件工具被关进沙箱，访问系统文件需显式配置 `GANYU_FS_ROOT`（或容器内挂载）。
- H3 Landlock 仅 Linux、且非强隔离；完整隔离仍需 Docker/gVisor（架构层面已预留 `sandbox` 模块与 hook）。
- C5 的 SSRF 防护无法纯客户端根除 DNS 重绑定，强隔离应在出口代理层。

### 验证（已通过）
- `cargo build` / `--features network` / `--features crypto,secret` / `--features shell` / `--features sandbox` / `--features hardened` 均编译通过。
- `cargo test`（默认 + crypto,secret + network）全绿：36（默认）/ 24（crypto,secret 含加密往返）/ 23（network） lib+集成+工作流测试通过。
- 复跑原始 PoC：exec 在默认构建未被识别（无 RCE）；file_read 解析到沙箱外失败（无泄漏）。
- `selftest` 9/9 通过；shell 特性在 `GANYU_ALLOW_SHELL=1` 下可正常执行、缺 env 时 Forbidden；默认 `tools` 列表不含插件。

## 后续
- L2 `hot_reload` 当前仅支持 OpenAI 类后端，后续可扩展为监听配置文件变更自动 reload。
- H3 建议在部署文档明确「生产用 Docker 运行 ganyu，挂载 `--volume` 作为沙箱根」，把 Landlock 升级为容器级隔离。
- C5 建议在生产前置一个 egress proxy（允许列表 + 强制解析校验），彻底消除 DNS 重绑定风险。
