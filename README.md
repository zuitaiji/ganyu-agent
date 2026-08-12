# ganyu-agent

> 有温度、能自进化、可拓展、可自愈的**完备 Agent 系统**（Rust 实现）。
> 会话 UUID 贯通 · 统一字符串值 · 抽象层驱动 · 默认离线可跑 · 安全失败闭环。

ganyu-agent 是一个从零构建的个人级/团队级 Agent 框架：覆盖对话执行与知识分析两条主线，
内置七大 agent 范式、ReAct 多步推理、自愈（重试/熔断/级联）、可生长技能，并在安全上
采取「**默认拒绝、显式开启**」的失败闭环策略。

## ✨ 特性

| 领域 | 能力 |
|------|------|
| **多范式** | 单 agent · ReAct · Plan & Execute · 多 agent · Router · Blackboard · Graph（统一 `Unit`/`RunContext`/`Workflow` 抽象） |
| **推理循环** | ReAct 感知→推理→行动→观察；可插拔 `Reasoner`；`@tool` 脚本与 JSON 原生函数调用（M6） |
| **自愈** | 重试+指数退避 · 熔断器 · 多后端级联 fallback · lkgp 粘路径 · 限速（令牌桶） |
| **记忆** | 5 层能力：会话轨迹（UUID 续接）· 语义检索（RAG 雏形）· 成功案例固化 · 失败沉淀 · 加密落盘（AES-256-GCM，可选） |
| **知识/分析** | SAG 五步管道（意图→上下文→生成→校验→自进化）+ MDL 本地语义校验 + SQL 注入防护 |
| **可拓展** | `tool!` 宏一行注册工具 · `plugins/*.json` 免重编译命令插件（白名单+校验） · `SkillBook` 可生长技能 |
| **安全** | 文件沙箱（C3/C4）· SSRF 防护 · shell 双层开关 · 插件默认关闭 · 输出净化 · 审计日志 · 启动基线自检 |
| **工程化** | 集中配置（`GANYU_*`）· LRU+TTL 缓存 · JSON Lines 审计 · 一键安装脚本 · 完整 ADR 决策记录 |
| **离线优先** | 默认构建零网络依赖（`LocalBackend`/`LocalReasoner` 兜底）；`network` 特性按需接入真实模型 |

## 🚀 快速开始

前置：Rust/cargo（[rustup.rs](https://rustup.rs)）。

```bash
# 一条命令安装（Linux/macOS/Git-Bash）
bash install.sh --features hardened
# 或 Windows PowerShell
.\install.ps1 -Features hardened

# 或直接从源码构建
cargo build --release --features hardened

# 自检 + 首跑
ganyu-agent selftest
ganyu-agent run "你好"
ganyu-agent sag "上月华东区利润最高的三个产品"
```

> 完整安装方式（`curl|sh` / `irm|iex` / cargo install / 特性矩阵 / 卸载）见 **[docs/install.md](docs/install.md)**。

## 📖 使用

```bash
ganyu-agent selftest                          # 内置自检（9 项）
ganyu-agent tools                             # 列出全部工具与特性技能
ganyu-agent modes                             # 列出七大范式
ganyu-agent run "@calc (1+2)*3"               # ReAct 推理循环（多步工具调用）
ganyu-agent agent "任务" --mode multi         # 多范式编排
ganyu-agent sag "上月华东区利润最高的三个产品"  # 知识分析（SAG）
echo "@calc 2+3" | ganyu-agent chat           # 对话面
```

> 全部子命令与示例见 **[docs/usage.md](docs/usage.md)**。

## 🏛 架构

```
接入层 ──→ 人格层(Pi-EQ SOUL) ──→ 推理循环 ReAct ──→ 路由层 Gateway(级联/熔断/lkgp/缓存/限速)
                  │                        │                     │
              Agent 编排 ────────── 工具层 ToolRegistry(内置/插件/技能) ──→ 记忆层 Memory
                  │                                                        │
             知识/分析面：SAG 管道 + MDL 校验             安全基件 security/ sandbox/
                  │                                       工程基件 config/ cache/ observe/
              自愈 heal：重试/熔断/级联/限速
```

- **统一抽象**：`Memory` / `LlmBackend` / `Tool` 三个 trait + `Gateway`(路由) + `Agent`(编排) + `heal`(自愈)；
  所有范式构建在同一 `Unit` 原子之上，`Workflow` 是对 `Unit` 的协调策略。
- **设计硬约束**：会话 UUID（`SessionId`）贯穿交互与记忆提交；统一字符串值（`Value(String)`）收敛全部载荷。
- 详细架构、数据流与模块职责见 **[docs/architecture.md](docs/architecture.md)**。

## 📁 目录结构

```
ganyu-agent/
├── src/
│   ├── main.rs / lib.rs      CLI 入口 / 库导出
│   ├── value.rs / error.rs / session.rs   统一值 / 统一错误 / 会话 UUID
│   ├── core/                抽象层与编排：llm / memory / agent / loop_(ReAct) / unit / workflow(7 范式)
│   ├── ext/                 能力面：工具注册 / 插件发现 / 技能（tool! / CommandTool / SkillBook）
│   ├── knowledge/           知识面：mdl(MDL 校验) / sag(SAG 管道)
│   ├── heal/                自愈：重试 / 熔断 / 级联 / 限速
│   ├── routing/             网关：级联 + lkgp + 熔断 + 缓存 + 审计
│   ├── security/            安全执行面：文件沙箱 / SSRF / shell 开关 / 输出净化
│   ├── sandbox.rs           进程级 FS 沙箱（Landlock，Linux-only）
│   ├── config.rs            工程化配置面（GANYU_* 集中 + 基线自检）
│   ├── cache.rs             缓存层（LRU+TTL）
│   ├── observe.rs           审计日志（JSON Lines）
│   └── persona/             人格层（Pi-EQ SOUL）
├── docs/                    文档体系（ADR 决策记录 / 指南 / 索引）
├── examples/                示例：sample_mdl.json / deploy-openviking.yml
├── plugins/                 免重编译命令插件示例
├── tests/                   集成测试（integration / workflows）
├── install.sh / install.ps1 一键安装脚本
├── SECURITY.md              安全治理基线
└── Cargo.toml               特性矩阵（network/crypto/secret/shell/sandbox/hardened）
```

## 📚 文档导航

| 文档 | 内容 |
|------|------|
| **[docs/README.md](docs/README.md)** | 文档总索引 + 代码地图 |
| **[docs/install.md](docs/install.md)** | 安装指南（一键脚本 / cargo / 源码 + 特性矩阵 + 卸载） |
| **[docs/usage.md](docs/usage.md)** | CLI 使用指南（全部子命令 + 示例） |
| **[docs/architecture.md](docs/architecture.md)** | 架构概览（分层 / 数据流 / 模块职责 / 扩展点） |
| **[docs/development.md](docs/development.md)** | 开发指南（构建 / 测试 / 特性 / 代码规范 / 贡献） |
| **docs/ADR-001 ~ 007** | 架构决策记录（统一架构 / 多范式 / 安全审计 / 修复落地 / 工程化 / 安装分发） |
| **[SECURITY.md](SECURITY.md)** | 安全治理基线（12 层防线 / env 清单 / 生产加固 / 漏洞报告） |

## 🔒 安全

默认姿态 = **失败闭环（fail-closed）**：默认不给任何能力，每一项都需显式开启。

| 能力 | 默认 | 开启方式 |
|------|------|----------|
| `exec` 本地执行 | 关 | `shell` 特性 + `GANYU_ALLOW_SHELL=1` |
| 插件发现 | 关 | `GANYU_ALLOW_PLUGINS=1` + `vetted:true` + 程序白名单 |
| 文件 IO | 沙箱内（`.ganyu_workspace`） | 相对路径；拒绝穿越/绝对路径/符号链接逃逸 |
| `web_fetch` | 仅 network 特性 | 自动 SSRF 防护（禁内网/环回/云元数据） |
| 记忆落盘 | 明文 | `crypto` 特性 + `GANYU_MEM_KEY`（AES-256-GCM） |
| 缓存/限速/审计 | 关 | `GANYU_TOOL_CACHE_TTL` / `GANYU_RATE_PER_MIN` / `GANYU_AUDIT` |

> 完整防线、环境变量清单与生产加固组合见 **[SECURITY.md](SECURITY.md)**。

## 🛠 开发

```bash
cargo build                     # 默认构建（零网络依赖）
cargo build --features hardened # 生产加固组合（network+crypto+secret+shell）
cargo test                      # 全量测试（单元 + 集成 + 工作流）
cargo test --features crypto,secret   # 含记忆加密往返测试
```

> 特性矩阵、扩展方法（加工具/技能/后端）、代码规范与贡献流程见 **[docs/development.md](docs/development.md)**。

## 📜 来源与致谢

基于规划文档 `ganyu-agent.md` / `agent-fusion-architecture.md` / `model-routing-gateway-deploy.md`
落地；安全与工程化设计对标 2026 开源 agent（Pi / OpenClaw / Hermes / Prime 等，见 ADR-004/006）。
本仓库为 MIT 许可的个人/团队自托管 Agent 框架。
