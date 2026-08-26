# ganyu-agent 架构总览

> **最后更新：v0.1.15（2026-08-26）** — 本文是模块化 / 结构化权威快照，覆盖目录结构、分层、特性门控、能力边界矩阵、构建缓存、CI 发布链路。决策记录见 `docs/ADR-001~008`。
> 范式为：**Pi 式极简 harness** × **OpenClaw 式执行网关** × **Hermes 式防护与闭环** × **Prime 式诚实边界**。

---

## 0. 速览（TL;DR）

| 项 | 当前值 |
|----|--------|
| 形态 | 单 crate Rust（无 `[workspace]`） |
| 代码规模 | 39 个 `.rs` 文件 / 约 8970 行 / 201 个依赖条目 |
| 二进制 | `target/release/ganyu-agent` 3.7M（已 strip） |
| 特性门控 | **fail-closed**：`crypto`+`secret` 默认开；`network`/`shell`/`sign`/`sandbox` 默认关 |
| 能力探测 | `ganyu-agent doctor`（边界矩阵）+ `ganyu-agent capabilities`（JSON 导出） |
| 构建缓存 | 本地 sccache 对象级 + CI `Swatinem/rust-cache` 互补 |
| 发布 | tag `v*` 触发三平台 hardened 构建 + R-1 Ed25519 签名，9 资产 |
| 版本 | `v0.1.15`（Cargo.toml / Cargo.lock 同步） |

---

## 1. 目录结构与模块树（真实行数）

```
ganyu-agent/                         (根目录)
├── .cargo/
│   └── config.toml                  # sccache 对象级缓存 + CARGO_INCREMENTAL=0
├── .github/workflows/
│   └── release.yml                  # CI/CD：三平台构建+签名+发布（tag 触发）
├── src/                             # 全部 Rust 源码（39 文件 / ~8970 行）
│   ├── main.rs            1353     # CLI 入口（手写 match 分发子命令）
│   ├── config.rs           533     # 配置面：env + L2 Pi JSON 适配器（apply_pi_overrides）
│   ├── security.rs         563     # 执行面：文件沙箱/SSRF/shell 开关/净化（失败闭环）
│   ├── cache.rs            152     # LRU+TTL 只读工具缓存 / LLM 响应缓存
│   ├── observe.rs          120     # JSON Lines 审计日志
│   ├── release_sign.rs     210     # R-1 Ed25519 签名（sign 特性）/ 默认 stub 回退
│   ├── sandbox.rs           77     # Landlock 进程级沙箱（sandbox 特性，Linux-only）
│   ├── error.rs             63     # GanyuError 统一错误
│   ├── session.rs           29     # 会话 UUID（跨重启续接）
│   ├── value.rs             68     # 统一字符串值 Value
│   ├── lib.rs               28     # crate 根，导出模块
│   ├── core/           2087       # 内核层
│   │   ├── memory.rs        625     # 记忆；异步 IO；加密（crypto）；原子写
│   │   ├── loop_.rs         350     # 决策解析（@脚本 + JSON 函数调用）
│   │   ├── agent.rs         191     # ReAct 编排；失败作 Observation 回流
│   │   ├── llm.rs           203     # 后端抽象；5xx 可重试 / 4xx 致命分流
│   │   ├── unit.rs           93     # 可编排原子 Unit
│   │   ├── mod.rs            16
│   │   └── workflow/        609     # 7 种范式
│   │       ├── graph.rs     208     # 构造即校验环
│   │       ├── plan_execute.rs 101
│   │       ├── multi_agent.rs  56
│   │       ├── router.rs       90
│   │       ├── blackboard.rs   74
│   │       ├── mod.rs          45
│   │       └── single.rs       35
│   ├── ext/            2273       # 扩展层
│   │   ├── mod.rs           599     # ToolRegistry / SkillBook / 插件发现
│   │   ├── nomifun_caps.rs  575     # nomifun 技能桥接
│   │   ├── builtins.rs      536     # 内置工具（shell/network 门控）
│   │   ├── mcp.rs           377     # L1 MCP 客户端（stdio JSON-RPC 2.0）
│   │   └── skills.rs        186     # 特性技能注册 / 意图路由
│   ├── tools/           617        # 独立工具模块
│   │   ├── git_diff.rs     366     # git diff（network 路径门控）
│   │   ├── diagram.rs      214     # 架构图生成
│   │   ├── mod.rs           23
│   │   └── upper.rs         14
│   ├── knowledge/       445        # MDL/SAG 知识分析面
│   ├── routing/         317        # Gateway 网关路由（级联/熔断/lkgp/缓存/限速/审计）
│   ├── heal/            209        # 自愈重试/熔断/级联/限速
│   └── persona/          22        # 人格层
├── tests/                          # 集成测试（integration.rs / workflows.rs / mock_mcp_server.py）
├── docs/                           # 文档（ADR/架构/安全/配置指南/本文件）
├── skills/ plugins/ examples/ scripts/   # 资源目录
└── .ganyu_*.json / .ganyu_workspace/ / target/   # ⚠️ 运行时产物，已被 .gitignore 忽略
```

**结构原则**
- 内核 `core/` 与扩展 `ext/` 严格分层，扩展层通过 trait 接入内核，不反向依赖业务细节。
- `tools/` 为独立可编译单元（upper/diagram/git_diff/pr-diff），与 `ext/builtins` 的运行时注册工具区分。
- 运行时产物（`.ganyu_*` / `target/`）**不进版本库**——`.gitignore` 已覆盖，`git status` 始终干净。

---

## 2. 分层架构

```
接入层  CLI（run/chat/agent/sag/setup/update/model/models/gateway/tools/doctor/capabilities/selftest）
   │
编排层  Agent(ReAct) · Workflow(7 范式) · Unit + RunContext
   │
能力层  ToolRegistry（内置 tool! / 插件 CommandTool / 技能 SkillBook / MCP mcp:*）
   │
记忆层  Memory（LocalMemory / OpenVikingMemory，会话 UUID 轨迹）
   │
模型层  Gateway（级联+熔断+lkgp+缓存+限速+审计）→ LlmBackend
   │
知识层  SAG 管道 + MDL 语义校验
   │
横切层  heal（自愈）· security（执行面）· sandbox（进程级）· config/cache/observe（工程面）
```

**一次请求旅程（ReAct）**
```
消息 → 技能路由(match_intent) → Reasoner.decide
     → tools.call（只读:重试+缓存 / 副作用:不缓存不重试 / 安全:沙箱·SSRF·shell 开关在此执行）
     → Observation 回流 → 循环(MAX_STEPS=8) → Final
     → memory.commit(session, trace)  // UUID 轨迹落盘，跨重启续接（Prime 会话树）
```

**配置加载链（含 L2 生态适配）**
```
GanyuConfig::from_env()          # 读 GANYU_* 环境变量（零依赖配置层，对标 Pi「配置即文件」）
   → apply_pi_overrides()        # L2：读 ~/.ganyu/settings.json 叠加覆盖（fail-closed，缺失静默跳过）
   → 启动 CLI / Agent
```

---

## 3. 特性门控矩阵

定义在 `Cargo.toml [features]`。**默认 `default = ["crypto", "secret"]`** —— 密钥零化与记忆加密默认可用，避免「忘了 `--features hardened` 就裸奔」。

| 特性 | 依赖 | 默认 | 关闭的影响 |
|------|------|------|-----------|
| `crypto` | aes-gcm, rand, sha2 | ✅ 开 | 记忆加密不可用（H1 防线失效） |
| `secret` | zeroize | ✅ 开 | 密钥零化不可用（L1 防线失效） |
| `network` | reqwest, ring | ❌ 关 | web_fetch / http_call / git remote diff / OpenAI 后端全部不编译 |
| `shell` | （无额外依赖，仅编译开关） | ❌ 关 | `shell_exec` 工具不编译进二进制 |
| `sign` | ring, ed25519-dalek | ❌ 关 | R-1 发布签名不编译（CI 用 `hardened` 实际启用） |
| `sandbox` | landlock | ❌ 关 | Landlock 进程沙箱不编译（仅 Linux 生效） |
| `hardened` | = network+crypto+secret+shell | ❌ 关 | CI 生产构建的便捷组合（不含 sandbox，跨平台隔离由文件沙箱兜底） |

**release profile（小体积）**
```toml
[profile.release]
lto = "thin"          # 增量 LTO，代价小
codegen-units = 4     # 适度聚合
strip = true          # 去符号
panic = "abort"       # 已验证源码无 catch_unwind
```

---

## 4. 能力边界矩阵（核心痛点：能力可探测）

此前「能力无法探测边界」——已通过 `doctor` 边界矩阵 + `capabilities` JSON 子命令解决。映射源在 `main.rs::capability_matrix()`（`cfg!` 编译期判定，声明式、零侵入）。

| 能力 | 模块 | 所需特性 | 默认启用 | 来源 |
|------|------|----------|----------|------|
| memory read/write | core/memory | always | ✅ | builtin |
| skill:*（特性技能） | ext/skills | always | ✅ | skill |
| nomifun:*（桥接） | ext/nomifun_caps | always | ✅ | skill |
| tool:upper/diagram/git_diff(pr) | tools/ | always | ✅ | builtin |
| shell_exec | ext/builtins | shell | ❌ | builtin |
| web_fetch | ext/builtins | network | ❌ | builtin |
| http_call | ext/builtins | network | ❌ | builtin |
| git remote diff | tools/git_diff | network | ❌ | builtin |
| openai backend | core/llm | network | ❌ | builtin |
| memory encrypt (H1) | core/memory | crypto+secret | ✅ | builtin |
| efface key (L1) | core/memory | crypto+secret | ✅ | builtin |
| release sign (R-1) | release_sign | sign | ❌ | builtin |
| landlock sandbox | sandbox | sandbox | ❌ | builtin |
| mcp:*（MCP 客户端） | ext/mcp | runtime: `GANYU_ALLOW_MCP=1` | ❌（env） | mcp |
| plugin:*（插件发现） | ext/mod | runtime: `GANYU_ALLOW_PLUGINS=1` | ❌（env） | plugin |

> **运行时门控**（非编译特性）：MCP 客户端与插件发现不靠编译开关，而靠环境变量 fail-closed 门控——未显式放行即拒绝加载。

**如何使用**
```bash
ganyu-agent doctor         # 输出「能力边界矩阵」段：一眼看清关掉某特性会少哪些能力
ganyu-agent capabilities   # JSON 导出完整能力清单（程序化探测边界）
```

---

## 5. 构建与缓存优化

**项目级 `.cargo/config.toml`**（仅项目级生效，不污染用户全局配置）
```toml
[build]
rustc-wrapper = "sccache"        # 对象级编译缓存

[env]
CARGO_INCREMENTAL = "0"          # 关键：否则 sccache 判定 non-cacheable（坑 1）
SCCACHE_CACHE_SIZE = "10G"       # 本地磁盘缓存上限
```

**验证结论（v0.1.15 前实测）**
- 两次不同 `target-dir` 编译同一项目，第二次命中 **116 个缓存对象（42.65%）** —— sccache 跨目录复用生效。
- 命中率上限由 **proc-macro 派生宏 crate（syn/serde_derive）天生不可缓存** + 单 crate 自身决定，属 sccache 固有限制，不属缺陷。
- **不拆 workspace 提命中率**：单 crate 拆 workspace 是大重构、ROI 极低、引入跨 crate API 风险，已明确拦住。

**CI 缓存共存**
- CI 沿用 `Swatinem/rust-cache@v2`（按 `CARGO_TARGET_DIR` 缓存），与本项目级 sccache（按编译对象缓存）**互补、互不冲突**。
- CI 额外 `cargo install sccache --locked`，使 `rustc-wrapper` 在 GitHub runner 也可见（否则项目级 config 会让 CI 因缺 wrapper 构建失败 —— 已修）。

---

## 6. CI/CD 发布链路

**触发**：`release.yml` 仅在 `push: tags: ["v*"]` 时运行（推 `main` 不触发 CI，故每次发布前必打 tag）。

**actions 版本（已升级至 Node 24 运行时，消除 Node.js 20 deprecated 警告）**
| action | 版本 | 用途 |
|--------|------|------|
| actions/checkout | `@v7` | 拉取代码 |
| dtolnay/rust-toolchain | `@stable` | 安装 Rust |
| Swatinem/rust-cache | `@v2` | 依赖缓存 |
| cargo install sccache | `--locked` | 安装编译缓存（配合项目级 config） |
| actions/upload-artifact | `@v7` | 上传构建产物 |
| actions/download-artifact | `@v8` | 下载产物（与 upload 同属现代 artifact 后端，跨大版本互通已验证） |
| softprops/action-gh-release | `@v3` | 发布 Release + 资产 |

**流水线（每平台 ubuntu / macos / windows）**
```
build(hardened) → Test → Security audit → Smoke → Archive → Checksum(.sha256) → Sign(Ed25519 R-1) → Upload → Publish Release
```

**发布资产（v0.1.14 / v0.1.15 均为 9 资产）**
```
linux   : ganyu-agent-linux.tar.gz  + .sha256 + .sig
macos   : ganyu-agent-macos.tar.gz  + .sha256 + .sig
windows : ganyu-agent-windows.tar.gz + .sha256 + .sig
```
`.sig` 为 R-1 Ed25519 供应链签名，`verify_update_signature` 强校验更新完整性。

---

## 7. 扩展点（对标 Pi 原语哲学）

1. **加工具**：`reg.register(crate::tool!(name, "描述", closure))`
2. **加插件（免重编译）**：`plugins/*.json` + `vetted:true` + 白名单（`GANYU_ALLOW_PLUGINS=1` 启用）
3. **加技能**：`SkillBook::register_skill(Skill{steps})` → 自动注册 `skill:<name>` 并支持意图路由
4. **接模型/记忆**：实现 `LlmBackend` / `Memory`，注册进 `Gateway` / `Agent`
5. **加能力边界项**：在 `main.rs::capability_matrix()` 追加一行 `CapabilityRow`（保持边界矩阵与代码同步）

---

## 8. 已知限制与后续

- **本地构建受腾讯电脑管家实时防护拦截**（os error 5 写锁 + 并行编译 SIGKILL）—— 验证用 temp 副本 + `--offline` + 单作业绕过；**CI 是跨平台权威验证门**。
- **能力边界矩阵为声明式手动维护**：新增特性门控能力时需在 `capability_matrix()` 同步加行（已在函数注释标注）。
- **生态兼容路线图**：L1 MCP 已落地，L2 Pi 配置适配器已落地；L3（Prime RLM 范式 / OpenClaw 多平台网关）为既定未完方向。
- **Node 24 升级已完成**（v0.1.15），CI 日志 `Node.js 20` 警告归零。
