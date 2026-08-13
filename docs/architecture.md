# ganyu-agent 架构概览

> 以对标范式组织：**Pi 式极简 harness** × **OpenClaw 式执行网关** × **Hermes 式防护与闭环** × **Prime 式诚实边界**。
> 决策记录见 ADR-001~008。

## 1. 定位：四种范式如何落到本项目

| 对标范式 | 借鉴点 | 本项目落地 |
|----------|--------|-----------|
| **Pi**（极简 harness） | 原语而非功能；技能=文件可版本化；多运行模式 | `Unit`/`tool!` 宏（一行注册原语）；`SkillBook` 技能=代码结构；CLI 多模式（run/agent/chat/sag） |
| **OpenClaw**（执行网关） | 多后端网关 + 热路径缓存复用 + 崩溃可恢复存储 | `Gateway` 级联/lkgp/熔断；`cache` LRU+TTL（只读工具+LLM 响应）；memory 原子写（tmp+rename） |
| **Hermes**（闭环学习 + 防护） | 自进化技能 + 分层记忆 + 多道防线 | `SkillBook` 成功固化/失败沉淀（自进化）；会话轨迹+检索+案例（记忆分层）；12 层安全防线（对应 Hermes 8 层） |
| **Prime**（诚实边界） | 会话树/续接；明确「进程隔离≠安全沙箱」 | 会话 UUID 跨重启续接（`--session`）；文档明示沙箱根≠容器隔离，强隔离用 Docker/gVisor |

## 2. 分层

```
接入层 CLI（run/chat/agent/sag/setup/update/model/models/gateway/tools/selftest/doctor）
   │
编排层 Agent(ReAct) · Workflow(7 范式) · Unit + RunContext
   │
能力层 ToolRegistry（内置 tool! / 插件 CommandTool / 技能 SkillBook）
   │
记忆层 Memory（LocalMemory / OpenVikingMemory，会话 UUID 轨迹）
   │
模型层 Gateway（级联+熔断+lkgp+缓存+限速+审计）→ LlmBackend
   │
知识层 SAG 管道 + MDL 语义校验
   │
横切层 heal（自愈）· security（执行面）· sandbox（进程级）· config/cache/observe（工程面）
```

## 3. 一次请求的旅程（ReAct）

```
消息 → 技能路由(match_intent) → Reasoner.decide
     → tools.call（只读:重试+缓存 / 副作用:不缓存不重试 / 安全:沙箱·SSRF·shell 开关在此执行）
     → Observation 回流 → 循环(MAX_STEPS=8) → Final
     → memory.commit(session, trace)  // UUID 轨迹落盘，跨重启续接（Prime 会话树）
```

## 4. 核心抽象

| 抽象 | 职责 | 默认 | 扩展点 |
|------|------|------|--------|
| `Memory` | URI→值；会话轨迹 | `LocalMemory`（可加密） | 实现 trait |
| `LlmBackend` | 对话补全 | `LocalBackend`（离线兜底） | `OpenAiBackend`（network） |
| `Tool` | 原子能力 | 内置 10+ | `tool!` / 插件 |
| `Reasoner` | 单步决策（async） | `LocalReasoner`（离线）/ `LlmReasoner`（配置模型自动启用） | 实现 trait |
| `Unit` | 可编排原子 | `Agent` | 任意 Unit |
| `Workflow` | 协调策略 | 7 范式 | 新范式实现 trait |
| `Gateway` | 后端路由 | 级联+熔断+lkgp（**本地兜底永远排最后**） | register/hot_reload |

## 5. 模块职责速查

| 模块 | 职责 |
|------|------|
| `core/llm.rs` | 后端抽象；5xx 可重试/4xx 致命分流 |
| `core/memory.rs` | 记忆；异步 IO；加密（crypto）；原子写 |
| `core/agent.rs` | ReAct 编排；失败作 Observation 回流 |
| `core/loop_.rs` | 决策解析（@脚本 + JSON 函数调用） |
| `core/unit.rs` / `workflow/` | Unit + 7 范式（Graph 构造即校验环） |
| `ext/` | 工具/插件/技能；副作用标注 |
| `knowledge/` | MDL 校验 + SQL 注入检测；SAG 管道 |
| `heal/` | 重试/熔断/级联/限速 |
| `routing/` | 网关 + 缓存 + 审计 + 输出净化 |
| `security.rs` | 文件沙箱/SSRF/shell 开关/净化（失败闭环） |
| `sandbox.rs` | Landlock（Linux-only） |
| `config.rs` | 配置 + 基线自检 + config.toml 读写（model/gateway 段） |
| `cache.rs` / `observe.rs` | LRU+TTL 缓存 / JSON Lines 审计 |
| `main.rs` CLI | 子命令分发；`setup`（交互向导）/`update`（release 自更新）/`model`（切换）/`gateway`（Telegram 长轮询） |

## 6. 扩展点（对标 Pi 原语哲学）

1. 加工具：`reg.register(crate::tool!(name, "描述", closure))`
2. 加插件（免重编译）：`plugins/*.json` + `vetted:true` + 白名单
3. 加技能：`SkillBook::register_skill(Skill{steps})` → 自动注册 `skill:<name>` 并支持意图路由
4. 接模型/记忆：实现 `LlmBackend`/`Memory`，注册进 `Gateway`/`Agent`
