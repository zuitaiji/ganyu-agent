# ganyu-agent 安装指南

> 三种方式：一键脚本（推荐）· cargo install（开发者）· 源码构建（贡献者）。
> 前置：Rust/cargo（[rustup.rs](https://rustup.rs)）。默认构建零外部依赖、离线可装。

## 1. 一键脚本

```bash
# Linux / macOS / Git-Bash（仓库内）
bash install.sh --features hardened
# 远程（替换为你的直链）
curl -fsSL https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.sh | bash

# Windows PowerShell（仓库内）
.\install.ps1 -Features hardened
# 远程
iex (irm https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.ps1)
```

脚本行为：检测 cargo → 定位/克隆源码 → `cargo install --locked --root <prefix>`
（构建目录在 `$prefix\target` 持久缓存，幂等升级增量编译）→ selftest 自检 → 创建 `ganyu` 别名 → PATH 提示。

参数：`--features`（默认空=默认构建；生产 `hardened`）、`--prefix`（sh 默认 `~/.local`，ps1 默认 `~\.ganyu`）、
`--branch` / `--repo`、`--no-alias`、`-Dev`（ps1：dev profile 快装，验证用）。

## 2. cargo install（开发者/CI）

```bash
cargo install --git https://github.com/zuitaiji/ganyu-agent.git --branch main \
  --features hardened --locked
cargo install --path . --root "$HOME/.local" --features hardened --locked   # 本地
```

## 3. 源码构建（贡献者）

```bash
git clone https://github.com/zuitaiji/ganyu-agent.git && cd ganyu-agent
cargo build --release --features hardened
cargo test
./target/release/ganyu-agent selftest
```

## 4. 开箱即用（装完就能对话）

```bash
# 1. 自检与诊断
ganyu-agent selftest
ganyu-agent doctor          # 环境/配置/网关/能力面一键体检

# 2. 写一次配置文件（OpenAI 兼容端点均可）
#    ~/.ganyu/config.toml
#    [model]
#    base_url = "https://api.openai.com/v1"
#    api_key = "sk-..."
#    model = "你的模型id"

# 3. 直接对话（交互式 REPL，多轮上下文延续；/quit 或 Ctrl+C 退出）
ganyu-agent chat
```
> 配置后 `run`/`agent`/`sag` 也自动走真实模型；未配置则离线本地兜底（功能不缺失）。

## 5. 特性矩阵

| 特性 | 能力 | 代价 |
|------|------|------|
| 默认 | 全功能本地兜底 | 无联网/无加密 |
| `network` | 真实 LLM / web_fetch / OpenViking | reqwest+rustls |
| `crypto` | 记忆 AES-256-GCM | 3 个小型 crate |
| `secret` | API key 内存清零 | zeroize |
| `shell` | exec（双层放行） | 本地执行面 |
| `sandbox` | Landlock（仅 Linux） | landlock |
| `hardened` | 以上除 sandbox 的全部 | 构建最久 |

## 6. 卸载

```bash
rm -f "$HOME/.local/bin/ganyu-agent" "$HOME/.local/bin/ganyu"        # sh
Remove-Item "$HOME\.ganyu\bin\ganyu-agent.exe", "$HOME\.ganyu\bin\ganyu.exe" -Force   # ps1
```
> 数据（记忆文件、沙箱根）保留在工作目录，卸载不影响。

## 7. FAQ

| 问题 | 处理 |
|------|------|
| 没有 cargo | 先装 rustup 再跑脚本 |
| 安装慢 | 默认构建 ~30 crate；hardened 首次较久（之后增量） |
| Windows 报毒 | 脚本仅执行 `cargo install` 与官方命令；可将 `~\.ganyu` 加 Defender 排除 |
| 脚本被 PATH 同名劫持 | 在仓库目录内用 `./install.sh` |
