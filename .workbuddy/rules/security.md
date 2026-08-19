# Security 规则（ganyu-agent，R.A.I.L.G.U.A.R.D.）

> 涉及密钥/沙箱/审计/执行面/供应链的改动，本文件为强制前置阅读。

## 硬规则（不可违反）

- **永不硬编码凭据**：API key/token/passphrase/私钥不进代码、注释、日志、文档、提交
- **永不读取/打印/暴露** `.env`、私钥文件、`~/.ganyu/config.toml` 中的敏感字段
- **密钥零化**：内存中的密钥用 `zeroize`（secret 特性）擦除；不用时即释放
- **记忆加密**：落盘记忆含敏感信息必须走 `crypto` 特性 + `GANYU_MEM_KEY`；**密钥错误时禁止覆盖密文文件**（load_failed 保护，见 src/core/memory.rs）
- **禁止不安全执行**：生产代码不用 `eval` 式执行；exec/shell 需 `shell` 特性 + `GANYU_ALLOW_SHELL=1` 双保险

## 特性与运行时门控

| 能力 | 编译门 | 运行门 | 说明 |
|---|---|---|---|
| exec/shell | `--features shell` | `GANYU_ALLOW_SHELL=1` | C1 失败闭环：双门缺一即禁 |
| 插件 | — | `GANYU_ALLOW_PLUGINS=1` + `GANYU_PLUGIN_ALLOW` 白名单 | C2：白名单外全拒 |
| 文件访问 | — | `GANYU_FS_ROOT` 沙箱根 | C3/C4：进程内路径校验 |
| 记忆加密 | `crypto`（默认开） | `GANYU_MEM_KEY` | 无密钥时明文落盘（P3，用户须知） |

## 数据面安全

- **审计日志**（observe.rs）：只记工具名/状态/耗时，**禁止记录 key/token/消息体**
- **缓存**：LLM 缓存值必须净化后（无敏感信息）；键不含明文消息
- **输入验证**：SAG/知识面 SQL 为 mock 执行；真实执行面必须在信任边界验证输入
- **SSRF 意识**：管理员配置的 base_url 属信任边界；从不可信来源构造 URL 前必须校验

## 供应链

- 依赖审计：改动 Cargo.toml 后跑 `cargo audit`（如有）；锁文件随版本提交
- 自更新（update）：依赖 GitHub release 的 sha256 验签，禁用降级路径的裸执行
- 签名：release 资产走 `scripts/sign-release.py` 签名（ed25519）

## 报告与应急

- 发现疑似泄露的真实密钥 → 立即提醒用户轮换，并从未提交历史/日志中清除
- 安全面改动评审：P0（漏洞/泄露）立即停手上报；P1 需修复后过审；P2 记录并安排修复
- 高风险组合（如 shell 开但无容器隔离）→ 启动基线自检告警，不得静默
