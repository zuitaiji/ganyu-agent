# ganyu-agent

> 有温度、能自进化、可拓展、可自愈的**完备 Agent 系统**（Rust），**开箱即用**。
> 写一次配置文件 → `ganyu-agent chat` 直接对话（OpenClaw/Hermes 式体验）。
> 范式对标：Pi（极简 harness）× OpenClaw（执行网关）× Hermes（防护+闭环）× Prime（诚实边界）。

## 特性

| 领域 | 能力 |
|------|------|
| **开箱即用** | 一条命令安装（免编译下载 release）→ `ganyu setup` 一键配模型 → 交互式 REPL 对话；`doctor` 环境诊断；`update` 自更新 |
| 多范式 | single / react / plan / multi / router / blackboard / graph（统一 `Unit`×`Workflow`） |
| 推理 | ReAct 循环；`LlmReasoner` 接真模型（`@tool` 自动行动）；`@tool` 脚本 + JSON 原生函数调用 |
| 自愈 | 重试+退避 · 熔断 · 级联 · lkgp · 限速 |
| 记忆 | 会话 UUID 轨迹续接 · 检索 · 案例固化/失败沉淀（自进化）· AES-256-GCM 加密（可选） |
| 知识 | SAG 五步管道 + MDL 校验 + SQL 注入防护 |
| 可拓展 | `tool!` 宏 · 免重编译插件（白名单）· 可生长技能 |
| 安全 | 文件沙箱 · SSRF 防护 · shell 双层开关 · 输出净化 · 审计 · 基线自检（12 层防线） |
| 工程化 | 集中配置 · LRU+TTL 缓存 · JSON 审计 · CI 自动发布 · ADR 决策记录 |
| 离线优先 | 默认零网络依赖；`network` 特性接真模型 |
| nomifun 赋能 | 全量接入 nomifun 平台 **33 项内置 agent 能力**（代码/测试/安全/视频/设计/架构/调试等），注册为 `skill:<name>` 并进入意图路由；离线返回方法论 SOP，设 `GANYU_NOMIFUN_GATEWAY` 走真实桥接（详见 `docs/nomifun_capabilities.md`） |

## 快速开始（开箱即用）

```bash
# 1. 安装（Hermes 式一条命令，免编译下载 release；详见 docs/install.md）
curl -fsSL https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.sh | bash   # Linux/macOS/Git-Bash
iex (irm https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.ps1)          # Windows

# 2. 配置模型（交互式向导，回车沿用当前值）
ganyu setup

# 3. 直接对话（交互式 REPL，多轮上下文延续；/quit 或 Ctrl+C 退出）
ganyu chat
```

> 模型已配置时 `run`/`agent`/`sag` 自动走真实模型；未配置则离线本地兜底，功能照常。

## 使用速查

```bash
ganyu chat                 # 交互式对话（推荐入口，等同 ganyu-agent chat）
ganyu setup                # 交互式配置模型（base_url/api_key/model 写入 config.toml）
ganyu model                # 查看当前模型；ganyu model <新模型id> 切换
ganyu models               # 查询网关全部可用模型（需已配置端点）
ganyu doctor               # 环境诊断（配置/特性/网关/能力面）
ganyu update               # 从 GitHub Releases 自动升级到最新版
ganyu run "你好"            # 单次对话
ganyu run "@calc (1+2)*3"  # 工具（离线可用）
ganyu agent "任务" --mode multi   # 多范式
ganyu gateway start        # 接 Telegram 消息平台（可选）
ganyu tools | modes | selftest    # 工具/范式/自检
```

## 架构（详见 docs/architecture.md）

```
CLI → Agent(ReAct, LlmReasoner) / Workflow(7范式) → ToolRegistry → Memory → Gateway → LlmBackend
         └──────── SAG + MDL（知识面）────────┘  横切：heal·security·sandbox·config·cache·observe
```

## 目录

```
src/     core(抽象编排) ext(能力) knowledge(知识) heal(自愈) routing(网关)
         security.rs sandbox.rs config.rs cache.rs observe.rs persona/
docs/    指南 + ADR-001~008（见总索引）
examples/  sample_mdl.json · deploy-openviking.yml
plugins/  命令插件示例
install.sh / install.ps1 · SECURITY.md · Cargo.toml（特性矩阵）
```

## 文档导航

| 文档 | 内容 |
|------|------|
| [docs/README.md](docs/README.md) | 文档总索引 + 代码地图 |
| [docs/install.md](docs/install.md) | 安装（一条命令免编译 / cargo / 源码 + 特性矩阵 + 开箱引导） |
| [docs/config-guide.md](docs/config-guide.md) | **配置模型指导**（setup 向导 + env 全量 + 场景模板） |
| [docs/usage.md](docs/usage.md) | CLI 使用（setup/model/update/gateway/REPL/doctor/全部子命令） |
| [docs/architecture.md](docs/architecture.md) | 架构（对标范式） |
| [docs/development.md](docs/development.md) | 开发/扩展/**发布流程（CI+tag）**/贡献 |
| [SECURITY.md](SECURITY.md) | 安全基线（12 层防线） |
| docs/ADR-001~008 | 架构决策记录 |

## 安全一句话

默认失败闭环：exec 关 / 插件关 / 文件沙箱 / SSRF 防护 / 加密可选 / 缓存·限速·审计默认关。
生产：`--features hardened` + 配置模板 C（见 config-guide）。
