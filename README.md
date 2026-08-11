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
接入层 ──→ 人格层(Pi-EQ SOUL) ──→ 路由层(Gateway: 级联+熔断+lkgp) ──→ 记忆层(Memory)
                 │                          │                              │
             Agent 编排 ──────────── 工具层(ToolRegistry / 插件) ──→ 进化层(SkillBook)
                 │
            知识/分析面：SAG 五步管道(意图→上下文→生成→校验→自进化) + MDL 语义校验
```

- **自愈（heal）**：重试+指数退避、熔断器、多后端级联 fallback、lkgp 粘成功路径；
  外部重服务（OpenViking / OmniRoute / 真 LLM）走适配器 + 本地降级，不可用时自动兜底。
- **可拓展（ext）**：`tool!` 宏一行注册工具；`plugins/*.json` 清单把外部命令注册为工具（**无需重编译**）；
  成功路径经 `SkillBook` 固化为技能（自进化），失败踪迹沉淀供规避。

## 构建与运行

```bash
cargo build            # 默认：零网络依赖即可编译运行（LocalBackend 兜底）
cargo test             # 10 个单元测试 + 集成测试

# 可选：接入真实 LLM（OpenAI 兼容端点，指向 OmniRoute / Ollama 等）
OPENAI_API_BASE=https://... OPENAI_API_KEY=sk-... cargo run --features network -- sag "..."

# 可选：记忆层接 OpenViking（:1933）；不可达时自动降级本地存储
OV_BASE=http://localhost:1933 cargo run -- sag "..."
```

### 子命令

```bash
cargo run -- sag "上月华东区利润最高的三个产品"   # 跑 SAG 管道（默认 examples/sample_mdl.json）
cargo run -- selftest                            # 内置自愈/可拓展自检
echo "@calc (1+2)*3" | cargo run -- chat         # 对话面（@name 分发到工具；无模型时本地兜底）
```

## 如何扩展

1. **加工具（编译期）**：`reg.register(crate::tool!(my_tool, "描述", |i: &Value| -> GanyuResult<Value> { ... }));`
2. **加工具（运行期，免重编译）**：在 `plugins/example.json` 加一个 `{name, command, description}`，
   后端自动 `discover` 把外部命令注册为工具（stdin 收输入，stdout 作返回值）。
3. **接真实模型 / 记忆**：实现 `LlmBackend` / `Memory` trait，注册进 `Gateway` / `Agent` 即可，
   默认 `LocalBackend` / `LocalMemory` 作为自愈兜底始终保留。

## 目录

```
src/
  value.rs      统一字符串值 Value
  error.rs      统一错误 GanyuError（thiserror）
  session.rs    会话 UUID SessionId
  heal/         自愈：重试 / 熔断 / 级联 fallback
  core/         llm(Memory后端) / memory / agent(编排)
  routing/      Gateway：级联 + 熔断 + lkgp
  ext/          Tool 抽象 / tool! 宏 / 命令插件 / SkillBook
  persona/      Pi-EQ 人格 SOUL
  knowledge/     mdl(MDL 校验) / sag(SAG 五步管道)
examples/        sample_mdl.json / deploy-openviking.yml（来自原规划备份）
plugins/         命令插件示例
tests/           集成测试
```
