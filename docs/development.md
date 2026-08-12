# ganyu-agent 开发指南

> 面向开发者/贡献者。配置模型见 [config-guide.md](config-guide.md)，架构见 [architecture.md](architecture.md)。

## 构建

```bash
cargo build                          # 默认：零网络依赖
cargo build --features hardened      # 生产：network+crypto+secret+shell
cargo build --features network       # 接真实 LLM / web_fetch
cargo build --features crypto,secret # 记忆加密 + key 清零
cargo build --features shell         # 编译 exec（运行时仍需 GANYU_ALLOW_SHELL=1）
cargo build --features sandbox       # Landlock（Linux-only，目标特定依赖）
```

## 测试

```bash
cargo test                                  # 全量（单元+集成+工作流）
cargo test --features crypto,secret         # 含加密往返
cargo test --features network               # 含 SSRF/缓存路径
ganyu-agent selftest                        # CLI 自检
```

安全属性必须有测试：沙箱拒穿越/绝对路径、SSRF 拦内网、只读缓存命中+副作用不缓存、
LLM 缓存吸收重复调用、SQL 注入检测、加密落盘密文且错密钥读空库。

## 特性矩阵

| 特性 | 能力 | 代价 |
|------|------|------|
| 默认 | 全功能本地兜底 | 无联网/无加密 |
| `network` | OpenAI 兼容/WebFetch/OpenViking | reqwest+rustls |
| `crypto` | 记忆 AES-256-GCM | aes-gcm/sha2/rand |
| `secret` | API key 内存清零 | zeroize |
| `shell` | exec（双层放行） | 本地执行面 |
| `sandbox` | Landlock 进程沙箱 | landlock（Linux） |
| `hardened` | network+crypto+secret+shell | 构建最久（生产推荐） |

## 扩展（Pi 式原语）

1. **工具**：`reg.register(crate::tool!(name, "描述", |i: &Value| -> GanyuResult<Value> {...}))`
   ——副作用工具实现 `Tool::side_effecting()=true`（不缓存、不盲目重试）。
2. **插件**（免重编译）：`plugins/*.json` 加 `{name,command,description,"vetted":true}`，
   运行时 `GANYU_ALLOW_PLUGINS=1` + `GANYU_PLUGIN_ALLOW` 白名单。
3. **技能**：`SkillBook::register_skill(Skill{name,description,steps})`，steps=Call/Note/Summarize。
4. **模型/记忆**：实现 `LlmBackend`/`Memory`，注册进 `Gateway`/`Agent`，`LocalBackend`/`LocalMemory` 兜底保留。

## 工程约定

- 安全失败闭环：新能力默认关闭，显式开关（特性+env 双保险）。
- 副作用标注 + 不缓存；模型输出经 `sanitize_model_output`。
- 新增 env 先登记 `src/config.rs::ENV_DOCS`；审计事件走 `observe::AuditLog`。
- 架构取舍先写 ADR（Context/Decision/Consequences）再动代码。
- 提交前：`cargo test`（相关特性）+ `cargo build --features hardened` 无新增 warning。

## 贡献流程

Fork → 分支 → 取舍先 ADR → 编码+测试 → 全绿 → PR（说明动机与验证证据）。

## 已知边界

- 默认无真模型，语义质量需 `network`；
- `sandbox` 仅 Linux；强隔离用 Docker/gVisor（SECURITY.md §部署）；
- SSRF 无法纯客户端根除 DNS 重绑定（出口代理做强隔离）；
- Windows 下含中文 `.ps1` 需 UTF-8 BOM（install.ps1 已处理）。
