# ganyu-agent 安装指南

> 三条路径：**一键脚本**（推荐）· **cargo install**（开发者）· **本地源码构建**（贡献者）。
> 设计决策见 [ADR-007](ADR-007-install-distribution.md)。

## 0. 前置要求
- **Rust / cargo**（1.70+）：<https://rustup.rs>
- 默认构建**零外部依赖、离线可装**；`hardened` 等特性才会拉取网络/TLS 依赖。

## 1. 一键脚本安装（推荐）

### Linux / macOS / Git-Bash
```bash
# 本地（仓库内）
bash install.sh --features hardened

# 一条命令（远程）——替换为你的仓库/发布直链
curl -fsSL https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.sh | bash
```

### Windows（PowerShell 5.1+）
```powershell
# 本地（仓库内）
.\install.ps1 -Features hardened

# 一条命令（远程）
iex (irm https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.ps1)
```

脚本会：检测 cargo → 定位/克隆源码 → `cargo install --root <prefix>`（构建目录在系统临时区，
不污染工作区）→ 跑 `selftest` 自检 → 创建 `ganyu` 别名 → 提示 PATH。

常用参数：

| 参数 | 说明 | 默认 |
|------|------|------|
| `--features <f1,f2>` / `-Features` | 特性组合（见下表） | 空（默认构建） |
| `--prefix <dir>` / `-Prefix` | 安装前缀 | `~/.local`（sh）/ `~\.ganyu`（ps1） |
| `--branch <b>` / `-Branch` | 源码分支 | `main` |
| `--repo <url>` / `-Repo` | 仓库地址 | GitHub 主仓库 |
| `--no-alias` / `-NoAlias` | 不创建 `ganyu` 别名 | 创建 |

## 2. 特性矩阵（能力按需开启，默认失败闭环）

| 特性 | 能力 | 代价 |
|------|------|------|
| （无，默认） | 离线可用：7 大范式 / 工具 / 记忆 / 自愈 | 无联网模型、无加密 |
| `network` | OpenAI 兼容后端（OpenAI/Ollama/OmniRoute）、web_fetch、OpenViking 记忆 | 引入 reqwest/rustls |
| `crypto` | 记忆 AES-256-GCM 加密落盘（需 `GANYU_MEM_KEY`） | aes-gcm/sha2/rand |
| `secret` | API key 内存清零（zeroize） | zeroize |
| `shell` | 编译 `exec` 工具（运行时仍需 `GANYU_ALLOW_SHELL=1`） | 本地执行面 |
| `sandbox` | exec 子进程 Landlock FS 沙箱（**仅 Linux**） | landlock |
| `hardened` | network + crypto + secret + shell（生产推荐组合） | 构建时间最长 |

## 3. cargo install（开发者/CI）
```bash
# 从仓库安装（GitHub 直装需 cargo 1.79+）
cargo install --git https://github.com/zuitaiji/ganyu-agent.git --branch main \
  --features hardened --locked

# 从本地目录安装
cargo install --path . --root "$HOME/.local" --features hardened --locked
```

## 4. 本地源码构建（贡献者）
```bash
git clone https://github.com/zuitaiji/ganyu-agent.git
cd ganyu-agent
cargo build --release                      # 默认构建
cargo build --release --features hardened  # 生产加固
cargo test                                 # 全量测试
./target/release/ganyu-agent selftest      # 自检
```

## 5. 使用
```bash
ganyu-agent selftest                 # 自检（9 项）
ganyu-agent tools                    # 列出工具与技能
ganyu-agent run "你好"                # 对话/推理
ganyu-agent agent "任务" --mode multi # 多范式
ganyu-agent sag "上月华东区利润最高的三个产品"  # 知识分析
```

## 6. 卸载
```bash
# 脚本安装：删除二进制与别名即可
rm -f "$HOME/.local/bin/ganyu-agent" "$HOME/.local/bin/ganyu"
# Windows
Remove-Item "$HOME\.ganyu\bin\ganyu-agent.exe", "$HOME\.ganyu\bin\ganyu.exe" -Force
```
> 数据（记忆文件 `.ganyu_memory.json`、沙箱根 `.ganyu_workspace/`）保留在你的工作目录，
> 卸载二进制不影响数据。

## 7. 常见问题
- **没有 cargo**：先装 rustup（<https://rustup.rs>），再跑脚本。
- **安装慢**：默认构建只编译 ~30 个 crate；`hardened` 会拉 reqwest/rustls（首次较慢，之后增量）。
- **Windows Defender 报毒**：本脚本仅执行 `cargo install` 与官方命令，可从源码审查；
  建议将 `~\.ganyu` 与目标目录加入 Defender 排除项。
- **自定义特性**：`--features network,crypto,secret` 可任意组合（`shell,sandbox` 需 Linux）。
