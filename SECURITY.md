# ganyu-agent 安全治理基线（SECURITY.md）

> 治理面文档：如何理解本项目的安全边界、默认姿态、可开启能力与部署建议。
> 执行面实现见 `src/security.rs`（文件沙箱/SSRF/shell 开关）、`src/config.rs`（基线自检）、
> `src/observe.rs`（审计留痕）；设计决策见 `docs/ADR-003`（审计）、`ADR-005`（修复落地）、`ADR-006`（工程化/治理）。

## 0. 威胁模型与默认姿态

默认构建（`cargo build`，无 feature）面向**不可信输入环境**：

| 能力 | 默认 | 说明 |
|------|------|------|
| `exec` 本地执行 | **关闭** | 不编译进二进制（需 `shell` 特性）+ 运行时 `GANYU_ALLOW_SHELL=1` 双层放行 |
| 插件发现 | **关闭** | `GANYU_ALLOW_PLUGINS=1` + `vetted:true` + `GANYU_PLUGIN_ALLOW` 白名单 |
| 文件 IO | **沙箱内** | 默认根 `.ganyu_workspace`；拒绝绝对路径 / `..` / 符号链接逃逸 |
| `web_fetch` | 仅 network 特性 | 出站前 SSRF 防护（禁内网/环回/链路本地/云元数据），禁自动重定向 |
| 记忆落盘 | 明文 | `crypto` 特性 + `GANYU_MEM_KEY` 后为 AES-256-GCM 密文 |
| 缓存 / 限速 / 审计 | **关闭** | `GANYU_TOOL_CACHE_TTL` / `GANYU_LLM_CACHE_TTL` / `GANYU_RATE_PER_MIN` / `GANYU_AUDIT` 显式开启 |

**一句话**：默认不给任何能力；每一项都要你亲手开。

## 1. 分层防线（对标 Hermes 8 层 / OpenClaw Policy）

| 层 | 机制 | 状态 |
|----|------|------|
| L1 输入净化 | `sanitize_model_output`（去 NUL/控制字符、限长 1MB）在网关出口执行 | ✅ |
| L2 工具门控 | `Tool::side_effecting` + 只读缓存隔离；副作用工具不盲目重试（M3） | ✅ |
| L3 文件沙箱 | `resolve_sandboxed` 路径规范化 + 根内校验（C3/C4） | ✅ |
| L4 进程沙箱 | `sandbox` 特性 Landlock（Linux，exec 子进程内 pre_exec 施加） | ✅（Linux-only） |
| L5 SSRF 防护 | `ssrf_guard` 协议/域名/IP 四重校验（C5） | ✅ |
| L6 SQL 注入防护 | `Mdl::detect_injection` + 白名单模板构造（H2） | ✅ |
| L7 记忆加密 | AES-256-GCM（crypto 特性，H1） | ✅（opt-in） |
| L8 密钥保护 | `zeroize::Zeroizing`（secret 特性，L1） | ✅（opt-in） |
| L9 速率限制 | `Gateway::with_rate_limit` 令牌桶（M2） | ✅（opt-in） |
| L10 审计留痕 | `observe::AuditLog` JSON Lines（Pi ledger / OpenClaw 留证） | ✅（opt-in） |
| L11 配置治理 | `config::security_baseline` 启动自检高危组合 | ✅ |
| L12 容器隔离 | Docker/gVisor（推荐生产） | 📌 部署层，见 §4 |

## 2. 环境变量清单（单一来源：`src/config.rs::ENV_DOCS`）

| 变量 | 作用 | 默认 |
|------|------|------|
| `GANYU_FS_ROOT` | 文件沙箱根目录 | `.ganyu_workspace` |
| `GANYU_MEM_KEY` | 记忆加密 passphrase（crypto 特性） | 无 |
| `GANYU_ALLOW_SHELL` | `=1` 放行 exec（需 shell 特性） | 关 |
| `GANYU_ALLOW_PLUGINS` | `=1` 启用插件发现 | 关 |
| `GANYU_PLUGIN_ALLOW` | 插件程序名白名单（逗号分隔） | 空（全拒） |
| `GANYU_TOOL_CACHE_TTL` | 只读工具结果缓存 TTL（毫秒） | 0（关） |
| `GANYU_LLM_CACHE_TTL` | LLM 响应缓存 TTL（毫秒） | 0（关） |
| `GANYU_RATE_PER_MIN` | 网关每分钟请求上限 | 0（不限） |
| `GANYU_AUDIT` | `1`/`stderr`/文件路径 开启审计 | 关 |
| `OV_BASE` | OpenViking 记忆服务地址（network） | 无 |
| `OPENAI_API_BASE` / `OPENAI_API_KEY` | OpenAI 兼容后端（network） | 无 |

## 3. 生产加固推荐组合

```bash
# 编译：全部加固（不含 Linux-only sandbox；在 Linux 上可再加 --features sandbox）
cargo build --release --features hardened
# 运行时（示例）
export GANYU_MEM_KEY='<16+ 字符强口令>'
export GANYU_RATE_PER_MIN=60
export GANYU_TOOL_CACHE_TTL=30000
export GANYU_LLM_CACHE_TTL=60000
export GANYU_AUDIT=1
```

启动时 `security_baseline` 会给出高危组合告警（如 shell 开但无容器隔离）。

## 4. 部署建议（诚实边界，对齐 Prime「隔离≠沙箱」）

- 文件沙箱（C3/C4）与 Landlock（H3）是**轻量第一道防线**，不是强隔离。
- 生产建议 **Docker 运行 ganyu**：`docker run --volume /srv/ganyu-workspace:/workspace ...`，
  把沙箱根映射为容器挂载卷；敏感任务叠加 gVisor 或独立主机。
- `exec` 建议默认不启用；确需时使用一次性容器 + 严格 `GANYU_PLUGIN_ALLOW`。
- SSRF 强隔离需在**出口代理**层做（允许列表 + 强制解析校验），客户端防护无法根除 DNS 重绑定。
- 多用户部署：本系统为单租户设计（对齐 Hermes 单租户假设），多租户隔离应在 OS/容器层完成。

## 5. 漏洞报告

发现安全问题请提供最小复现（输入 + 特性组合 + 环境），按 `docs/ADR-003` 的 PoC 方式记录，
修复将走 ADR-005 的失败闭环原则（默认拒绝 → 显式开启）。
