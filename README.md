# ganyu-agent

> 有温度、能自进化、可拓展、可自愈的**完备 Agent 系统**（Rust）。
> 会话 UUID · 统一字符串值 · 抽象层 · 默认离线 · 安全失败闭环。
> 范式对标：Pi（极简 harness）× OpenClaw（执行网关）× Hermes（防护+闭环）× Prime（诚实边界）。

## 特性

| 领域 | 能力 |
|------|------|
| 多范式 | single / react / plan / multi / router / blackboard / graph（统一 `Unit`×`Workflow`） |
| 推理 | ReAct 循环；`@tool` 脚本 + JSON 原生函数调用 |
| 自愈 | 重试+退避 · 熔断 · 级联 · lkgp · 限速 |
| 记忆 | 会话 UUID 轨迹续接 · 检索 · 案例固化/失败沉淀（自进化）· AES-256-GCM 加密（可选） |
| 知识 | SAG 五步管道 + MDL 校验 + SQL 注入防护 |
| 可拓展 | `tool!` 宏 · 免重编译插件（白名单）· 可生长技能 |
| 安全 | 文件沙箱 · SSRF 防护 · shell 双层开关 · 输出净化 · 审计 · 基线自检（12 层防线） |
| 工程化 | 集中配置 · LRU+TTL 缓存 · JSON 审计 · 一键安装 · ADR 决策记录 |
| 离线优先 | 默认零网络依赖；`network` 特性接真模型 |

## 快速开始

```bash
# 安装（详见 docs/install.md）
bash install.sh --features hardened        # Linux/macOS/Git-Bash
.\install.ps1 -Features hardened           # Windows
# 自检 + 首跑
ganyu-agent selftest
ganyu-agent run "你好"
ganyu-agent sag "上月华东区利润最高的三个产品"
```

## 使用速查

```bash
ganyu-agent tools | modes                  # 工具 / 范式
ganyu-agent run "@calc (1+2)*3"            # ReAct
ganyu-agent agent "任务" --mode multi      # 多范式
ganyu-agent skill summarize path           # 技能
```

## 架构（详见 docs/architecture.md）

```
CLI → Agent(ReAct) / Workflow(7范式) → ToolRegistry → Memory → Gateway → LlmBackend
         └──────── SAG + MDL（知识面）────────┘  横切：heal·security·sandbox·config·cache·observe
```

## 目录

```
src/     core(抽象编排) ext(能力) knowledge(知识) heal(自愈) routing(网关)
         security.rs sandbox.rs config.rs cache.rs observe.rs persona/
docs/    指南 + ADR-001~007（见总索引）
examples/  sample_mdl.json · deploy-openviking.yml
plugins/  命令插件示例
install.sh / install.ps1 · SECURITY.md · Cargo.toml（特性矩阵）
```

## 文档导航

| 文档 | 内容 |
|------|------|
| [docs/README.md](docs/README.md) | 文档总索引 + 代码地图 |
| [docs/install.md](docs/install.md) | 安装（脚本/cargo/源码 + 特性矩阵） |
| [docs/config-guide.md](docs/config-guide.md) | **配置模型指导**（env 全量 + 场景模板） |
| [docs/usage.md](docs/usage.md) | CLI 使用 |
| [docs/architecture.md](docs/architecture.md) | 架构（对标范式） |
| [docs/development.md](docs/development.md) | 开发/扩展/贡献 |
| [SECURITY.md](SECURITY.md) | 安全基线（12 层防线） |
| docs/ADR-001~007 | 架构决策记录 |

## 安全一句话

默认失败闭环：exec 关 / 插件关 / 文件沙箱 / SSRF 防护 / 加密可选 / 缓存·限速·审计默认关。
生产：`--features hardened` + 配置模板 C（见 config-guide）。
