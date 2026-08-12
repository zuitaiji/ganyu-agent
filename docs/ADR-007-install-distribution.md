# ADR-007: 安装与分发策略（一键脚本 / cargo / 源码）

## Status
Accepted

## Context
ganyu-agent 已具备完整能力与安全闭环，但缺少「可安装性」：用户只能 `cargo run`，无法一行装好、
进 PATH、快速体验。2026 主流的个人 agent 分发都验证了两种模式：
- **curl|sh / irm|iex**（Pi `curl -fsSL https://pi.dev/install.sh | sh`、Hermes `curl|bash`、
  OpenClaw 官方脚本）：零门槛，但供应链风险高（ClawHavoc 恶意技能事件、Hermes 官方「先审查再跑」）。
- **cargo install**（Rust 生态标准）：可信、可锁版本，但对非 Rust 用户门槛高。

本项目是 Rust 单体 CLI（无复杂运行时），无独立分发管道（无 CI 发布二进制），
因此采用「**脚本引导 + cargo install 内核**」：一键脚本负责环境检测、源码获取、特性选择、
PATH 与自检，实际安装动作仍是 `cargo install`（复用 Rust 生态信任链与 `--locked` 锁文件）。

## Decision

### D1 安装内核 = `cargo install --path/--git --locked --root <prefix>`
- 不引入预编译二进制管道（无 CI/签名基础设施时，二进制分发反而制造信任问题——对标 ClawHub 教训）；
- `--locked` 保证按仓库锁定的依赖版本构建，可复现。
- 构建目录默认放到系统临时区（`CARGO_TARGET_DIR`），避免污染用户工作区 `target/`。

### D2 一键脚本（install.sh / install.ps1）承担「引导」职责
- 检测 cargo → 无则提示 rustup（不代装，尊重用户环境）；
- 源码定位：脚本在仓库内则用之，否则 `git clone --depth 1 --branch main`；
- 特性默认「空=默认构建」（离线零依赖、秒装），`--features hardened` 供生产显式选择；
- 安装后自动 `selftest` 自检 + 创建 `ganyu` 别名 + PATH 提示；
- 幂等：重复执行即覆盖升级，不动 `.ganyu_workspace/` 与记忆数据。

### D3 供应链安全（对标 Hermes「先审查再跑」/ ClawHub 扫描）
- 脚本本身保持**最小面**：只做环境检查、git clone（HTTPS）、cargo install；无 curl|sh 远程管道执行代码；
- 远程使用时要求 HTTPS 直链；发布产物（未来二进制）必须附带 `.sha256` 并在脚本中校验；
- 文档明示「运行任何远程安装脚本前先审查」；安装脚本可逐行审计。

### D4 特性矩阵与默认值（延续失败闭环）
- 默认构建 = 零网络依赖、离线可装、无 exec/无插件/无加密；
- `hardened = network + crypto + secret + shell` 作为生产推荐组合（不含 Linux-only sandbox）；
- 文档提供 `docs/install.md` 特性矩阵与卸载/FAQ。

## Consequences

### 变容易的事
- 用户一条命令安装、一条命令升级，PATH/自检/别名开箱即用；
- 特性选择显式化，生产部署有明确推荐组合（hardened）；
- 开发者仍可用标准 `cargo install --git`，两套方式并存不冲突。

### 变困难 / 成本
- 无预编译二进制 → 首次安装需编译（默认 ~1-3 分钟，hardened 首次更久）；
- 无签名/版本发布管道 → 依赖 git 分支而非语义化发布（后续可加 GitHub Actions 发布 + .sha256）；
- 远程一键脚本存在供应链信任问题，靠「最小面 + HTTPS + 审查文档」缓解。

## 后续
- 接入 CI（GitHub Actions）发布 release 二进制 + `.sha256`，脚本支持「有产物则下载校验、否则回退编译」；
- 增加 `ganyu doctor`（对标 OpenClaw）检测环境/特性/锁文件健康；
- 提供 Homebrew / scoop / cargo-binstall 渠道（有 release 产物后）。
