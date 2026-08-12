# ganyu-agent 开发指南

> 面向开发者/贡献者：如何构建、测试、扩展，以及工程约定。
> 架构背景见 [architecture.md](architecture.md)，决策记录见 ADR-001~007。

## 1. 环境与构建

前置：Rust/cargo（[rustup.rs](https://rustup.rs)，1.70+）。

```bash
cargo build                          # 默认构建：零网络依赖，离线可跑
cargo build --release                # 发布构建
cargo build --features hardened      # 生产加固：network+crypto+secret+shell
cargo build --features network       # 接入真实 LLM / web_fetch / OpenViking
cargo build --features crypto,secret # 记忆加密 + 密钥清零
cargo build --features shell         # 编译 exec（运行时仍需 GANYU_ALLOW_SHELL=1）
cargo build --features sandbox       # 进程级 Landlock FS 沙箱（仅 Linux 拉取依赖）
```

## 2. 测试

```bash
cargo test                           # 全量：单元 + 集成（integration）+ 工作流（workflows）
cargo test --features crypto,secret  # 含记忆加密往返、secret 特性路径
cargo test --features network        # 含 SSRF/缓存/网关路径
cargo test security::                # 安全基件测试（沙箱/SSRF/净化）
ganyu-agent selftest                 # CLI 内置自检（不依赖 cargo test）
```

测试覆盖关键安全属性：
- 沙箱拒绝 `../` 穿越、绝对路径；
- SSRF 拦截 127.0.0.1 / 169.254.169.254 / 192.168.x / `file://`；
- 只读工具结果缓存命中、**副作用工具永不缓存**；
- LLM 响应缓存吸收重复调用；
- SQL 注入检测（堆叠语句 / 危险 DDL / 注释截断）；
- 记忆加密：落盘为 `ENC1:` 密文、明文不泄露、错密钥读回空库。

## 3. 特性矩阵

| 特性 | 能力 | 代价 |
|------|------|------|
| （默认） | 全部范式/工具/记忆/自愈，本地兜底 | 无联网模型、无加密 |
| `network` | OpenAI 兼容后端 / web_fetch / OpenViking 记忆 | reqwest + rustls（TLS/C 依赖） |
| `crypto` | 记忆 AES-256-GCM 加密落盘（需 `GANYU_MEM_KEY`） | aes-gcm / sha2 / rand |
| `secret` | API key 内存清零（zeroize） | zeroize |
| `shell` | 编译 `exec` 工具（双层放行） | 本地执行面 |
| `sandbox` | exec 子进程 Landlock FS 沙箱（Linux-only） | landlock（目标特定依赖） |
| `hardened` | network + crypto + secret + shell | 构建时间最长（生产推荐） |

> `hardened` 不含 `sandbox`（跨平台保持绿色）；Linux 上可再加 `--features sandbox`。

## 4. 如何扩展

### 4.1 加一个内置工具（编译期）
```rust
reg.register(crate::tool!(my_tool, "工具描述", |i: &Value| -> GanyuResult<Value> {
    Ok(Value(format!("处理了 {}", i.as_str())))
}));
```
> 有副作用的工具请实现 `Tool::side_effecting() -> true`（避免盲目重试与缓存）。

### 4.2 加一个命令插件（运行期，免重编译）
1. 在 `plugins/*.json` 添加条目（`name`/`command`/`description`，并置 `"vetted": true`）；
2. 运行时 `GANYU_ALLOW_PLUGINS=1` 且 `GANYU_PLUGIN_ALLOW` 包含该程序名；
3. 命令经 stdin 收输入、stdout 作返回（`CommandTool`）。

### 4.3 加一个特性技能（可生长）
```rust
book.register_skill(Skill {
    name: "my_skill".into(),
    description: "做什么".into(),
    steps: vec![
        SkillStep::Call { tool: "file_read".into(), arg: "{input}".into() },
        SkillStep::Summarize { max_chars: 120 },
    ],
});
```
技能自动注册为 `skill:<name>` 工具，并能被自然语言意图关键字路由命中。

### 4.4 接真实模型 / 记忆 / 自愈
- 实现 `LlmBackend` trait，注册进 `Gateway`（级联/熔断/lkgp 自动接管）；
- 实现 `Memory` trait（或直接用 `LocalMemory` / `OpenVikingMemory`）；
- 复用 `heal::with_retry` / `CircuitBreaker` / `RateLimiter` 包装任意易错操作。

## 5. 工程约定

- **安全失败闭环**：新能力默认关闭，必须显式开关（特性 / 环境变量双保险）；
- **副作用工具标注**：`side_effecting()=true`，禁止缓存、禁止盲目重试；
- **输出净化**：模型输出经 `security::sanitize_model_output`（去 NUL/控制字符、限长 1MB）；
- **配置单一来源**：新增环境变量先登记到 `src/config.rs::ENV_DOCS`；
- **审计留痕**：工具调用/安全拒绝/网关级联等事件走 `observe::AuditLog`（JSON Lines）；
- **ADR 优先**：涉及架构取舍的变更先写 ADR（Context/Decision/Consequences），再动代码；
- **测试随行**：安全属性必须有测试（沙箱/SSRF/缓存/注入/加密）。

## 6. 贡献流程

1. Fork 仓库，`git checkout -b feat/xxx`；
2. 若涉及架构取舍：先补 `docs/ADR-0XX`；
3. 编码 → 补齐测试 → `cargo test`（默认 + 相关特性）全绿；
4. `cargo build --features hardened` 通过、无新增 warning；
5. 提交（见 [ADR-005](ADR-005-remediation-plan.md) 的失败闭环原则说明）；
6. 提 PR，说明改动动机与验证证据（对齐 ADR-006 的「发布日志 + 留证」）。

## 7. 已知边界

- 默认构建无真模型（本地兜底）；多范式/语义质量需接 `network` 才完整；
- `sandbox`（Landlock）仅 Linux；完整隔离建议 Docker/gVisor（见 SECURITY.md §4）；
- SSRF 防护无法纯客户端根除 DNS 重绑定，强隔离应在出口代理；
- Windows 环境注意：PowerShell 执行含中文的 `.ps1` 需 UTF-8 BOM；`install.ps1` 已处理。
