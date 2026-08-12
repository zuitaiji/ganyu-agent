# ganyu-agent 架构概览

> 面向架构读者：分层、数据流、模块职责与扩展点。
> 决策依据见 ADR-001（统一架构）/ ADR-002（多范式）/ ADR-005/006（安全与工程化）。

## 1. 分层总览

```
┌────────────────────────────── 接入层 ──────────────────────────────┐
│ CLI（run / chat / agent / sag / tools / selftest）· stdin 管道      │
└────────────────────────────────────────────────────────────────────┘
                          │
┌──────────────────────────▼──────────────────────────────────────────┐
│ 编排面：Agent（ReAct 推理循环，可插拔 Reasoner）· Workflow（7 范式） │
│   Unit 原子（name + run(ctx,input)）· RunContext（共享上下文/黑板）  │
├────────────────────────────────────────────────────────────────────┤
│ 能力面：ToolRegistry（内置/插件/技能）· SkillBook（可生长技能）      │
├────────────────────────────────────────────────────────────────────┤
│ 记忆面：Memory（LocalMemory / OpenVikingMemory，UUID 会话轨迹）     │
├────────────────────────────────────────────────────────────────────┤
│ 模型面：Gateway（多后端级联 + 熔断 + lkgp + 缓存 + 限速 + 审计）    │
│          LlmBackend（LocalBackend 兜底 / OpenAiBackend 可选）       │
├────────────────────────────────────────────────────────────────────┤
│ 知识面：SAG 管道（意图→上下文→生成→校验→自进化）· MDL 语义校验      │
├────────────────────────────────────────────────────────────────────┤
│ 横切：heal（重试/熔断/限速）· security（沙箱/SSRF/净化）·           │
│        sandbox（Landlock）· config（配置）· cache（LRU+TTL）·        │
│        observe（审计 JSON Lines）                                   │
└────────────────────────────────────────────────────────────────────┘
```

## 2. 一次请求的旅程（ReAct）

```
用户消息 ──► Agent.run(msg)
              │ 1. 技能路由：自然语言意图 → SkillBook.match_intent（可选）
              │ 2. Reasoner.decide(msg, known)：
              │      · LocalReasoner：解析 @tool 脚本 / JSON 函数调用
              │      · （未来 LlmReasoner：模型产出动作）
              │ 3. 命中工具 → tools.call(name, args)
              │      · 只读工具：重试自愈 + 可选 LRU 缓存
              │      · 副作用工具：不缓存、不盲目重试
              │      · 安全拦截：沙箱/SSRF/shell 开关在此层执行
              │ 4. Observation 回流 → 继续循环（MAX_STEPS=8）
              │ 5. Final → 作答
              └─► memory.commit(session, trace)   // UUID 轨迹落盘（跨重启续接）
```

## 3. 核心抽象（trait）

| 抽象 | 职责 | 默认实现 | 扩展点 |
|------|------|----------|--------|
| `Memory` | 命名空间 URI → 值；会话轨迹提交/续接 | `LocalMemory`（JSON 落盘，可加密） | 实现 trait 接任意存储 |
| `LlmBackend` | 对话补全 | `LocalBackend`（离线兜底） | `OpenAiBackend`（network） |
| `Tool` | 原子能力 | 内置 10+ 工具 | `tool!` 宏 / `CommandTool` 插件 |
| `Reasoner` | 单步决策 | `LocalReasoner` | 接模型后替换 |
| `Unit` | 可编排原子 | `Agent` | 任意 `Unit` |
| `Workflow` | 对 `Unit` 的协调策略 | 7 种范式 | 新范式=实现 trait |
| `Gateway` | 多后端路由 | 级联+熔断+lkgp | `register` / `hot_reload` |

## 4. 模块职责表（src/）

| 模块 | 职责 | 关键约束 |
|------|------|----------|
| `core/llm.rs` | 模型后端抽象与 OpenAI 兼容实现 | network 特性；错误分流（5xx 可重试/4xx 致命） |
| `core/memory.rs` | 记忆：Local/OpenViking、会话轨迹、加密 | crypto 特性；异步 IO；原子写 |
| `core/agent.rs` | 编排：ReAct 循环、角色、会话续接 | MAX_STEPS=8；失败作 Observation 回流 |
| `core/loop_.rs` | 推理循环与决策 | `@tool` 脚本 + JSON 函数调用（M6） |
| `core/unit.rs` / `workflow/` | Unit 抽象与 7 范式 | 构造即校验（Graph 环检测） |
| `ext/` | 工具注册 / 插件发现 / 技能 | 插件默认关 + 白名单；副作用标注 |
| `knowledge/mdl.rs` | MDL 语义校验 + SQL 注入检测 | 表/列存在性 + 危险关键字 |
| `knowledge/sag.rs` | SAG 五步管道 | 模板降级自愈；自进化写回 |
| `heal/` | 重试 / 熔断 / 级联 / 限速 | 指数退避；令牌桶 |
| `routing/` | 网关：级联 + lkgp + 缓存 + 审计 | 输出净化出口；热更新 |
| `security.rs` | 文件沙箱 / SSRF / shell 开关 / 净化 | **失败闭环默认拒绝** |
| `sandbox.rs` | Landlock 进程沙箱（Linux） | 目标特定依赖，仅 Linux |
| `config.rs` | GANYU_* 集中配置 + 基线自检 | ENV_DOCS 单一来源 |
| `cache.rs` | LRU+TTL 缓存 | 副作用工具永不缓存 |
| `observe.rs` | JSON Lines 审计 | GANYU_AUDIT 开关 |
| `persona/` | Pi-EQ 人格 SOUL | system prompt 注入 |

## 5. 关键设计决策（ADR 索引）

| 决策 | 记录 |
|------|------|
| 会话 UUID + 统一字符串值 + 抽象层 | ADR-001 |
| Unit/RunContext/Workflow 三层抽象（7 范式） | ADR-002 |
| 缺陷/漏洞全量审计（5C/3H/6M/2L + PoC） | ADR-003 |
| 2026 开源 agent 对标（防护边界） | ADR-004 / ADR-006 |
| P0–P3 修复落地（失败闭环） | ADR-005 |
| 工程化：配置/缓存/审计/目录 | ADR-006 |
| 安装与分发（脚本 + cargo + 供应链安全） | ADR-007 |

## 6. 安全边界速览

- **执行面**（`security.rs`/`sandbox.rs`）：文件沙箱、SSRF、shell 双层开关、Landlock（Linux）。
- **数据面**：记忆加密（crypto）、API key 清零（secret）、输出净化。
- **治理面**：启动基线自检（`config::security_baseline`）、审计留痕（`observe`）、漏洞报告（SECURITY.md §5）。
- **部署面**：容器隔离（Docker/gVisor）为强隔离建议，见 SECURITY.md §4。

> 完整防线表、env 清单见 **[SECURITY.md](../SECURITY.md)**。
