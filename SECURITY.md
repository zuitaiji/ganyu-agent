# ganyu-agent 安全治理基线

> 安全模型对标 **Hermes「8 层防线」+ OpenClaw「Policy 三类检查」+ Prime「诚实边界」**。
> 配置见 [docs/config-guide.md](docs/config-guide.md)；修复决策见 ADR-003/005；工程化见 ADR-006。

## 0. 威胁模型

默认构建面向**不可信输入**：默认不给任何能力，每项能力需显式开启（失败闭环 fail-closed）。

| 能力 | 默认 | 说明 |
|------|------|------|
| `exec` 本地执行 | 关 | 特性 `shell` + `GANYU_ALLOW_SHELL=1` 双层 |
| 插件发现 | 关 | `GANYU_ALLOW_PLUGINS=1` + `vetted` + 白名单 |
| 文件 IO | 沙箱内 | 默认根 `.ganyu_workspace`；拒穿越/绝对路径/符号链接 |
| `web_fetch` | network 特性 | 出站 SSRF 防护，禁自动重定向 |
| 记忆落盘 | 明文 | `crypto` + `GANYU_MEM_KEY` → AES-256-GCM |
| 缓存/限速/审计 | 关 | env 显式开启 |

## 1. 防线对照（Hermes 8 层 × ganyu 12 层）

| # | 防线 | 实现 |
|---|------|------|
| 1 | 输入净化 | `sanitize_model_output`：去 NUL/控制字符、限长 1MB（网关出口） |
| 2 | 工具门控 | `Tool::side_effecting`：副作用不缓存、不盲目重试（M3） |
| 3 | 文件沙箱 | `resolve_sandboxed`：路径规范化 + 根内校验（C3/C4） |
| 4 | 进程沙箱 | `sandbox` 特性 Landlock（Linux，exec 子进程内施加） |
| 5 | SSRF 防护 | 协议/域名/IP 四重校验；禁 169.254.169.254 等（C5）。**代理 fake-ip 豁免**：Clash 等代理把域名解析为 198.18.0.0/15 / fdfe:dcba:9876::/48 虚拟地址，该网段放行（连接经代理转发）；字面内网 IP 仍一律拒绝 |
| 6 | SQL 注入防护 | `Mdl::detect_injection` + 白名单模板构造（H2） |
| 7 | 记忆加密 | AES-256-GCM（crypto，H1） |
| 8 | 密钥保护 | `zeroize::Zeroizing`（secret，L1） |
| 9 | 速率限制 | 网关令牌桶（M2） |
| 10 | 审计留痕 | `observe::AuditLog` JSON Lines（工具/拒绝/级联/限速） |
| 11 | 配置治理 | `security_baseline` 启动自检高危组合 |
| 12 | 容器隔离 | Docker/gVisor（部署层，见 §3） |

## 2. 配置

环境变量全量清单、场景模板（开发/生产/容器）见 **[docs/config-guide.md](docs/config-guide.md)**。
`ganyu setup` 交互向导写入 `~/.ganyu/config.toml`（含 API key）；`ganyu gateway setup`
写入 Telegram bot token（`[gateway]` 段）——**两者均为敏感凭据，勿提交仓库/外传**；
生产从密钥管理器注入 env（`OPENAI_API_BASE/KEY` 优先于文件）。

生产加固一句话：
```bash
cargo build --release --features hardened
export GANYU_MEM_KEY='<≥16字符强口令>' GANYU_RATE_PER_MIN=60 GANYU_AUDIT=1
```

### 自更新签名（R-1，供应链强校验）

`ganyu update` 默认仅同源 sha256；配置固定 ed25519 公钥后升级为**发布方身份校验**
（防发布服务器/账号被接管投毒）。官方发布公钥与维护者签名/轮换流程见
**[docs/update-signing.md](docs/update-signing.md)**：

```
GANYU_UPDATE_PUBKEY=d2de2259cce226840e7acb743b89b98cf603d2781e7b1b5456855efe8bf02cec
```

> ✅ 2026-08-18 轮换后的**生产公钥**（演示公钥 `3875bdb…` 已作废）。
> 首次安装的 `install.sh`/`install.ps1` 也已硬编码该公钥并验签（信任锚点）。
> `GANYU_UPDATE_ALLOW_NOCHECK=1` 仅为完全离线/自托管场景的逃生舱口，不绕过签名校验。

## 3. 部署建议（诚实边界）

- 文件沙箱与 Landlock 是**第一道防线，不是强隔离**（对齐 Prime）；
- 生产用 **Docker**：`docker run -v /srv/workspace:/workspace -e GANYU_FS_ROOT=/workspace ...`，
  敏感任务叠加 gVisor；
- `exec` 默认不启用；确需时放入容器 + 严格白名单；
- SSRF 强隔离需**出口代理**（允许列表 + 强制解析），客户端无法根除 DNS 重绑定；
- 单租户设计（对齐 Hermes 单租户假设），多租户隔离在 OS/容器层完成。

## 4. 漏洞报告

提供最小复现（输入 + 特性组合 + 环境），按 ADR-003 的 PoC 方式记录；
修复遵循失败闭环原则（默认拒绝 → 显式开启）。
