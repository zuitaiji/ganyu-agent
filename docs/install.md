# ganyu-agent 安装指南

> 三种方式：一键脚本（推荐，默认免编译）· cargo install（开发者）· 源码构建（贡献者）。
> **免编译安装**：从 GitHub Releases 下载预编译 hardened 二进制，零 Rust 依赖，装到独立目录，删目录即卸载。

## 1. 一键脚本（Hermes 式一条命令）

```bash
# Linux / macOS / Git-Bash（远程一条命令）
curl -fsSL https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.sh | bash
# Windows PowerShell（远程一条命令）
iex (irm https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.ps1)
```

脚本行为（默认免编译）：GitHub API 查最新 release → 下载对应平台资产 → 解压到
`<prefix>/bin` → selftest 自检 → 创建 `ganyu` 别名 → PATH 提示。幂等：重复执行覆盖升级，不动 config.toml 与记忆文件。

参数（定制用）：
- **install.sh**：`bash install.sh --version v0.1.7 --prefix ~/.local --no-alias`；
  **指定 `--features hardened` 时回退源码编译**（本地有仓库用本地源码，否则 clone），适合要定制特性的开发者。
- **install.ps1**（`iex (irm ...)` 单行执行不支持脚本级参数，用环境变量）：
  ```powershell
  $env:GANYU_FEATURES = "hardened"   # 指定后回退 cargo 编译
  $env:GANYU_PREFIX   = "D:\ganyu"   # 安装前缀（默认 %USERPROFILE%\.ganyu）
  $env:GANYU_VERSION  = "v0.1.7"     # release 版本（默认 latest）
  $env:GANYU_DEV = "1"               # 源码编译用 dev profile
  $env:GANYU_NOALIAS = "1"           # 跳过别名
  iex (irm https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.ps1)
  ```

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

# 2. 配置模型（交互式向导，回车沿用当前值）
ganyu setup                 # 问 base_url / api_key / model → 写入 ~/.ganyu/config.toml

# 3. 直接对话（交互式 REPL，多轮上下文延续；/quit 或 Ctrl+C 退出）
ganyu-agent chat            # 或 ganyu
```
> 配置后 `run`/`agent`/`sag` 也自动走真实模型；未配置则离线本地兜底（功能不缺失）。

## 5. 卸载（三平台）

> 程序装于独立前缀目录，**删 bin 即卸载**，不动 config.toml 与记忆文件；
> 想彻底清除（含配置与记忆）再删前缀目录。默认前缀：Windows `~\.ganyu`，Linux/macOS `~/.local`。

```bash
# ---- Linux / macOS（install.sh 默认前缀 ~/.local）----
rm -f ~/.local/bin/ganyu-agent ~/.local/bin/ganyu          # 卸程序（精确删，勿删整个 ~/.local）
rm -rf ~/.local/.ganyu*                                    # 可选：清测试残留
# 彻底清除（含 ~/.ganyu/config.toml 与记忆，重装需重新 setup）：
rm -rf ~/.ganyu ~/.local/.ganyu*

# ---- Windows（PowerShell）----
Remove-Item "$HOME\.ganyu\bin" -Recurse -Force              # 卸程序，保留 config/记忆
Remove-Item "$HOME\.ganyu" -Recurse -Force                  # 彻底清除（先备份 config.toml 的 API key）
```

> 自定义了 `GANYU_PREFIX`/`PREFIX` 的，按实际前缀替换上面的路径。

## 6. 特性矩阵

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
