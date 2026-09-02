# 更新日志

版本遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

发布机制：推送 `v*` tag 触发 `release.yml`，自动构建三平台二进制、
生成 SHA256 校验与 Ed25519 签名（R-1），并发布 GitHub Release。
GitHub 侧的 Release Notes 由 `generate_release_notes` 自动生成；
本文件是仓库内的可读变更历史，按版本倒序排列。

## [v0.1.20]

### 文档与安全（v0.1.19 发布后补齐的 main 提交，本版本一并带入发布）
- `docs/architecture.md` 刷新至 v0.1.19：补 `.github/workflows/ci.yml` 与 `src/gateway/`（512 行）模块树、§6 双工作流分工、§8 L3 网关「已落地」、真实规模 42 文件 / 10675 行。
- `docs/SECURITY-REPORT.md` 补 HTTP Webhook 桥接端点攻击面（v0.1.17 新攻击面）：§1.1 专项威胁子节、§3 R-10（High→Low，fail-closed 降级）、§5 残余接受（明文 HTTP 交 TLS 反向代理）。

### 缺陷修复
- `core/memory.rs`：`search()` 按 f64 score 排序的 `partial_cmp().unwrap()` 在 NaN 时理论 panic，在 `panic = "abort"` 常驻进程下会拖垮整个 agent；改 `unwrap_or(Ordering::Equal)`（与 v0.1.17 gateway 治理同源）。核实：memory.rs 共 21 处 unwrap，20 处在 `#[cfg(test)]` 断言（保留），生产路径仅此 1 处。

## [v0.1.19]

### 测试

此前 `src/tools/`（912 行）零单测、`src/routing/` 仅 3 个单测，补全 11 个，覆盖此前裸露的关键路径：

- **`src/tools/diagram.rs`（4 个）**
  - `esc` 的 XML 转义与**顺序不变量**：`&` 必须先转义，否则 `&lt;` 会被二次转义成 `&amp;lt;`；
    注入载荷 `</text><script>` 转义后不得残留裸 `<` / `>`（SVG 是 XML，未转义即注入）。
  - `box_node` 多行标签逐行生成 `text`、`bold` 仅影响 `font-weight`。
  - `edge` 的虚线开关与空标签不生成元素。
  - 端到端：生成的两个 SVG 须声明命名空间、闭合、`<svg>` 标签配对。
- **`src/tools/git_diff.rs`（2 个）**：GitHub / GitLab 的 PR 链接解析逐字段断言，
  并覆盖反向用例（issues 链接、裸仓库、跨平台链接不得互配）。
- **`src/routing/mod.rs`（5 个）**：熔断达阈值后不再调用失效后端（此前完全未覆盖）、
  全后端失败返回聚合错误、`local` 兜底后端排序永远在真模型之后、
  模型输出在出口净化（NUL 与控制字符）、超 1 MiB 输出被拒绝。

## [v0.1.18]

### 缺陷修复

- **`strip_frontmatter` off-by-3 切片错误**（`src/ext/nomifun_caps.rs`）：
  剥离 SKILL.md 的 YAML frontmatter 时，先用 `trimmed[3..]` 定位结束标记，
  却用 `trimmed[end + 4..]` 按绝对位置切片，少偏移 3 导致正文残留 `---` 前缀，
  技能内容被污染。改用 `strip_prefix` 定位，索引基准一致；新增回归单测覆盖。
- **`await_holding_lock`**（`src/core/memory.rs`）：加密用例的 env 串行锁用
  `std::sync::Mutex` 且持锁期间 `await`，会阻塞 runtime 线程，poison 后在
  `panic = "abort"` 下拖垮整个测试进程。改用 `tokio::sync::Mutex`。

### 工程

- **clippy 存量告警清零**：33 处（含 tests）全部修复。机械项由 `cargo clippy --fix`
  处理（redundant_closure / manual_is_multiple_of / io::Error::other / needless_borrow /
  map_or / derivable_impls / let_unit_value 等）；其余人工处理：
  `LruCache::len` 补 `is_empty`、`PROD_PUBKEY` 按 `sign` 特性门控（其唯一使用点
  `cmd_seed_check` 同门控）、5 处 `doc_lazy_continuation` 改为独立段落。
- **`ci.yml` 的 clippy 转为阻断门禁**（`-- -D warnings`）：自 v0.1.18 起 lint 回归直接阻断，
  防止告警再次累积成债。

## [v0.1.17]

### 安全

- **HTTP 桥接鉴权加固**：`POST /message` 此前完全无鉴权、无请求体上限，
  而 `hardened` 构建下 agent 具备 shell / 文件 / MCP 工具权限——
  无鉴权暴露等同于把本机执行权交给网络上可达的任何人。
  - 绑定**非回环地址**且未配置 token → **拒绝启动**（fail-closed，不降级运行）；
  - 配置 token 后要求 `Authorization: Bearer <token>`，否则 `401`；
  - 请求体上限 64 KiB，超限 `413`；空文本 `400`；
  - 新增配置面 `GANYU_HTTP_TOKEN` / `[gateway] http_token`（L2 `settings.json` 亦可叠加）；
  - 非回环绑定在 `security_baseline()` 中产出告警。

### 可用性

- **常驻进程 panic 治理**：`profile.release` 为 `panic = "abort"`，
  而 `gateway start` 是长驻进程——一次 panic 会让所有平台适配器一起下线。
  - 队列 / 挂起表改用 `tokio::sync::Mutex`（异步锁无 poisoning，不再 `lock().unwrap()`）；
  - `reqwest::Client` 构建失败降级为 `Client::new()`，不再 `expect()`；
  - `run_adapter` 单次 `poll` / `send` 失败只记录并退避重试，不让适配器退出。

### 工程

- **新增 `ci.yml` 门禁**（PR + push main）：`cargo fmt --check`、`cargo test`、
  `cargo build --release --features hardened --locked`，clippy 以报告模式运行（见下）。
  此前 `release.yml` 仅在 tag 推送时运行，问题要到发布阶段才暴露——
  v0.1.16 首发的 `Cargo.lock` 缺 axum 依赖树事故即为此代价。
- 全量 `cargo fmt --all`，统一代码风格。

### 已知债

- clippy 存量告警 23 处（`doc_lazy_continuation` / `redundant_closure` /
  `manual_is_multiple_of` 等，均为机械可修的低风险项），分布在 9 个文件中。
  当前以**报告模式**运行以免门禁上线即红，清理后转为 `-D warnings` 阻断。

## [v0.1.16] - 2026-08-26

### 新增

- **L3 多平台网关适配器**：聊天平台接入抽象为 `PlatformAdapter` trait（`src/gateway/`）。
  - `TelegramAdapter`：裸 `reqwest` 调 Bot API（`getUpdates` 长轮询 + `sendMessage`）；
  - `HttpBridge`：axum 本地端点 `POST /message`，请求 / 响应桥接，零平台 SDK；
  - fail-closed：无凭据不启用；非 `network` 特性编译期排除；每 chat 独立 Agent 会话。
- `gateway setup <bot_token>` / `gateway start` 子命令；`http_bind` 配置面；
  能力矩阵新增 `gateway:telegram` / `gateway:http` 两行。
- `docs/l3-gateway-tech-spec.md`。

### 修复

- `Cargo.lock` 缺 axum 依赖树导致 CI `--locked` 三平台构建失败（首发失败后修复并 retag）。

## [v0.1.15] - 2026-08-26

- CI actions 升级至 Node 24 运行时，消除 Node.js 20 deprecated 警告。

## [v0.1.14] - 2026-08-25

- L2 生态兼容：Pi 风格 `settings.json` / `models.json` 叠加适配器。
- 能力边界探测：`doctor` 边界矩阵 + `capabilities` JSON 导出。
- 引入 sccache 编译对象缓存。

## [v0.1.13] - 2026-08-22

- 修复插件白名单空值被绕过（C2 fail-closed 失效）。
- MCP 客户端（L1）：ganyu 作为 MCP Host 经 stdio JSON-RPC 2.0 接入跨生态工具。
- R-1 签名 Rust 端 + tool 层调整。

## [v0.1.12] - 2026-08-20

- SAG 不再依赖 cwd：MDL 三级回退（`GANYU_MDL` → `examples/sample_mdl.json` → 内置模板）。
- 配置自愈：`~/.ganyu/config.toml` 缺失时自动生成模板（不含密钥）。
- 安全第二 / 第三阶段加固：记忆加密强化（R-2）、自更新 Ed25519 签名（R-1）、
  tar 盘符穿越（R-5）、权限收紧（R-8 / R-9）。
- CI 增加 `cargo audit` 供应链审计。
- 代理开发配置收拢至项目内 `.agents/`。

## [v0.1.11] - 2026-08-17

- `install.ps1` 主动注册用户级 PATH（幂等）+ 当前会话同步。
- 构建缓存优化（thin LTO / codegen-units / strip / panic=abort），二进制 7.31MB → 5.04MB。
- LLM 上下文前缀缓存（稳定 system 前缀 + 排序工具清单）。

## [v0.1.10] - 2026-08-17

- 修复加密记忆密钥错误时静默清空覆盖（永久丢失风险）。

## [v0.1.9] - 2026-08-16

- 修复 macOS 无 `sha256sum` 导致安装校验与 update 必然失败。

## [v0.1.8] - 2026-08-15

- 修复 `install.ps1` 在 `iex (irm ...)` 上下文的 `param()` 语法错误。
- 修复 F-10 后 `OPENAI_API_BASE` 兜底失效（已配置模型仍全走本地兜底）。

## [v0.1.7] - 2026-08-15

- SSRF 深度加固：IPv4 内嵌 IPv6 全变体拦截 + 连接层 IP 固定防 DNS 重绑定。

## [v0.1.6] - 2026-08-15

- 安全审计修复 F-01 / F-10 / F-11：密钥文件权限、命令执行超时与输出限制、calc 深度限制。

## [v0.1.5] - 2026-08-15

- `@tool` 多行参数并入止于下一个 `@` 行，保持多步语义。

## [v0.1.4] - 2026-08-15

- `selftest` 在 release 分发（无 `examples/`）下不再 panic，降级跳过。
- 新增 `--version` / `--help`（此前被当作 chat 进入 REPL）。

## [v0.1.3] - 2026-08-15

- sha256 供应链校验、gateway 会话隔离、掩码输入、首批单测。

## [v0.1.2] - 2026-08-14

- Hermes 式改造文档全量同步，Windows 资产名对齐，MSYS tar 自复制问题修复。

## [v0.1.1] - 2026-08-13

- Telegram 消息平台网关。

## [v0.1.0] - 2026-08-13

- 首个可用版本：会话 UUID + 统一字符串值 + 抽象层，
  ReAct 推理循环、七大 agent 范式、内置工具、自愈与可拓展框架。
