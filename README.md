# ganyu-agent

有温度、能自进化、可拓展、可自愈的完备 agent 系统（Rust 实现）。

基于规划文档 `ganyu-agent.md` / `agent-fusion-architecture.md` / `model-routing-gateway-deploy.md`
落地架构主干：**对话/执行面** + **知识/分析面**，由记忆层贯穿。

## 设计硬约束（用户要求）

| 约束 | 实现 |
|---|---|
| 会话 UUID | `SessionId(Uuid)`，贯穿 Agent / SAG / 记忆提交 |
| 统一数据类型为 string | `Value(String)` newtype，全链路载荷收敛为字符串（`From<&str/i64/f64/bool>`） |
| 抽象层 | `Memory` / `LlmBackend` / `Tool` 三个 trait + `Gateway`(路由) + `Agent`(编排) + `heal`(自愈) |
| 全量发挥 Rust | `enum` / `trait`+`dyn` / 泛型 `with_retry<F,T,E>` / `Result`+`?`+thiserror / `macro`(`tool!`) / `async`+tokio / 所有权 `Arc`+`Mutex`+`Send+Sync`+`OnceLock` |

## 架构

```
接入层 ──→ 人格层(Pi-EQ SOUL) ──→ 推理循环 loop(ReAct: 感知→推理→行动→观察) ──→ 路由层(Gateway)
                 │                          │                                    │
             Agent 编排 ──────────── 工具层(ToolRegistry / 插件 / 技能) ──→ 记忆层(Memory)
                 │                                                              │
            知识/分析面：SAG 五步管道(意图→上下文→生成→校验→自进化) + MDL 语义校验
```

- **统一 Unit 抽象（多范式基石）**：所有范式建立在同一个原子 `Unit`（`name` + `run(ctx,input)`）
  之上；`RunContext` 共享会话 UUID / 黑板 / 记忆 / 网关 / 工具 / 技能。ReAct 是 `Unit` 的内部行为，
  其余范式是对多个 `Unit` 的*协调策略*（`Workflow` trait，统一 `run(ctx,input)`）。
- **七大范式全覆盖**（见下节 + `docs/ADR-002-multi-paradigm.md`）：单 agent、ReAct、Plan & Execute、
  多 agent、Router、Blackboard、Graph Workflow。全部离线可跑，接真模型时同接口自动升级。
- **全量 agent 工作流（ReAct）**：`core/loop` 把单条消息升级为多步「思考→行动→观察」循环，
  由可插拔 `Reasoner` 驱动。`LocalReasoner` 离线解析 `@tool arg` 脚本 + 关键字路由到技能；
  接真模型时由 `LlmReasoner` 取代即自动进入多步深思。失败的工具调用作为 Observation 回流（自愈）。
- **自愈（heal）**：重试+指数退避、熔断器、多后端级联 fallback、lkgp 粘成功路径；
  外部重服务（OpenViking / OmniRoute / 真 LLM）走适配器 + 本地降级，不可用时自动兜底。
- **可拓展（ext）**：`tool!` 宏一行注册工具；`plugins/*.json` 清单把外部命令注册为工具（**无需重编译**）；
  `Skill` 把多步程序固化为可生长的特性技能；成功路径经 `SkillBook` 固化（自进化），失败踪迹沉淀供规避。

## 多范式 agent 框架

统一抽象：`Unit`（原子运行时）+ `RunContext`（共享上下文）+ `Workflow`（协调策略）。

| 范式 | CLI `--mode` | 协调语义 |
|------|------|----------|
| 单 agent | `single` | 直接跑一个 `Unit` |
| ReAct | `react` | `Unit`=Agent，内部多步循环 |
| Plan & Execute | `plan` | 规划器拆子任务 → 逐步执行 → 合成 |
| 多 agent | `multi` | 多 `Unit` 按轮次传递上下文 |
| Router | `router` | 分类器派发到专精 `Unit`，否则 fallback |
| Blackboard | `blackboard` | 各 `Unit` 写共享黑板，合成器读整块黑板 |
| Graph Workflow | `graph` | DAG 拓扑序执行，边传数据，构造即校验环 |

默认离线：规划用 `LocalPlanner`（按连接词拆分）、路由用 `KeywordRouter`（关键字），
无真模型时各范式输出本地兜底文本（机制已验证，语义需接模型才完整）。

## 构建与运行

```bash
cargo build            # 默认：零网络依赖即可编译运行（LocalBackend 兜底）
cargo test             # 28 个测试（单元 + 集成 + 工作流）

# 可选：接入真实 LLM（OpenAI 兼容端点，指向 OmniRoute / Ollama 等）
OPENAI_API_BASE=https://... OPENAI_API_KEY=sk-... cargo run --features network -- sag "..."

# 可选：记忆层接 OpenViking（:1933）；不可达时自动降级本地存储
OV_BASE=http://localhost:1933 cargo run -- sag "..."
```

### 子命令

```bash
cargo run -- run "@calc 2+3"                    # 跑完整 ReAct 推理循环（多步工具调用），打印轨迹与作答
cargo run -- run "@file_write a.txt\nhi\n@calc 1+1"   # 多步脚本：逐行执行，最后一行收尾
cargo run -- agent "任务" --mode plan          # 多范式：plan/react/multi/router/blackboard/graph/single
cargo run -- modes                             # 列出全部支持的范式
cargo run -- tools                             # 列出全部内置工具与特性技能
cargo run -- skill summarize path/to/file      # 直接调用特性技能
cargo run -- sag "上月华东区利润最高的三个产品"   # 跑 SAG 管道（默认 examples/sample_mdl.json）
cargo run -- selftest                          # 内置自愈/可拓展自检（9 项）
echo "@calc (1+2)*3" | cargo run -- chat       # 对话面（@name 分发；自然语言自动路由到技能；无模型时本地兜底）
cargo run -- chat --session <uuid>             # 续接指定会话（记忆中存在则注入上次轨迹，跨重启自进化）
```

> `chat` / `run` / `sag` 都会打印 `session: <uuid>`，复制该 UUID 即可用 `--session` 续接。

## 内置能力

**工具（`@tool` 或 `@name` 调用）**：`echo` `calc` `file_read` `file_write` `file_list`
`exec`（本机 shell）`remember` `recall` `rag_search`，以及 `web_fetch`（仅 `--features network`）。
`plugins/*.json` 里的外部命令也会被自动 `discover` 注册（如示例 `upper`）。

**特性技能（可生长，`skill:<name>` 调用，自然语言也能自动路由）**：
- `summarize` — 读文件并给离线摘要（行数/字符数 + 前若干字符）
- `troubleshoot` — 按报错/现象检索记忆中的成功案例与失败沉淀，给排查指引
- `kb_query` — 向记忆知识库提问（检索 + 摘要）

## 如何扩展

1. **加工具（编译期）**：`reg.register(crate::tool!(my_tool, "描述", |i: &Value| -> GanyuResult<Value> { ... }));`
2. **加工具（运行期，免重编译）**：在 `plugins/example.json` 加一个 `{name, command, description}`，
   后端自动 `discover` 把外部命令注册为工具（stdin 收输入，stdout 作返回值）。
3. **加特性技能（可生长）**：`book.register_skill(Skill { name, description, steps })`，
   `steps` 是 `Call{tool,arg}` / `Note{text}` / `Summarize{max_chars}` 的组合，无需改核心代码。
4. **接真实模型 / 记忆**：实现 `LlmBackend` / `Memory` trait，注册进 `Gateway` / `Agent` 即可，
   默认 `LocalBackend` / `LocalMemory` 作为自愈兜底始终保留。

## 近期深化（架构决策见 `docs/ADR-001-architecture.md`）

- **会话 UUID 真正贯通记忆**：修复 `Memory::commit` 原先 `SessionId::new()` 造随机会话的缺陷，改为显式传入真实会话；
  新增 `load_session` + `Agent::resume()`，支持 `--session <uuid>` 跨重启续接（自进化）。记忆层本身已文件持久化（`LocalMemory` 落盘 JSON，`SkillBook` 走它）。
- **真实 LLM 后端错误分流**：`OpenAiBackend`（`--features network`）加 30s 超时、复用 client，
  并把 5xx/408/429 映射为可重试的 `BackendUnavailable`、4xx 映射为致命的 `BackendError`，
  与网关熔断 + Agent 重试协同（只对"可重试"类生效，避免放大故障）。新增 `BackendError` 错误变体。
- **默认零网络依赖不变**：native 后端仍仅 `LocalBackend` 兜底；network 特性才引入 reqwest/TLS。

## 多范式框架（架构决策见 `docs/ADR-002-multi-paradigm.md`）

- 抽离 `Unit`（`name` + `run(ctx,input)`）+ `RunContext`（会话/黑板/记忆/网关/工具/技能共享）× `Workflow`
  协调策略，使单 agent / ReAct / Plan&Execute / 多 agent / Router / Blackboard / Graph 七大范式共用一套原子，
  零重复脚手架；`Agent` 实现 `Unit`（内部 ReAct，跑完写共享黑板 key=角色）。
- 离线可跑：`LocalPlanner`（连接词拆分）/ `KeywordRouter`（关键字路由）兜底；接真模型只需换 planner/router/reasoner。
- CLI：`agent "任务" --mode <...>` + `modes` 列举；28 个测试覆盖全部范式（含 Graph 环检测）。

## 目录

```
src/
  value.rs      统一字符串值 Value
  error.rs      统一错误 GanyuError（thiserror）
  session.rs    会话 UUID SessionId
  heal/         自愈：重试 / 熔断 / 级联 fallback
  core/         llm(模型后端) / memory / agent(编排) / loop_(ReAct 推理循环) / unit(Unit+RunContext) / workflow(七大范式)
  routing/      Gateway：级联 + 熔断 + lkgp
  ext/          Tool 抽象 / tool! 宏 / 命令插件 / SkillBook
  persona/      Pi-EQ 人格 SOUL
  knowledge/     mdl(MDL 校验) / sag(SAG 五步管道)
examples/        sample_mdl.json / deploy-openviking.yml（来自原规划备份）
plugins/         命令插件示例
tests/           集成测试
```
