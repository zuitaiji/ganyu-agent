# ganyu-agent 配置模型指导

> 配置哲学：**单一事实来源（`GANYU_*` env）+ 特性门控 + 启动基线自检**。
> 对标：Pi「配置即文件可版本化」→ ganyu 用 env 保持零依赖；OpenClaw「config 缓存复用」→ 本层集中读取一次。
> 代码实现：`src/config.rs`（类型化读取 + `security_baseline` + `ENV_DOCS`）。

## 1. 配置模型

```
编译期（特性门控）         运行期（环境变量）           生效位置
cargo --features shell ─┐
   network ─────────────┼──► GANYU_* 集中读取 ──► 执行面（security/memory/gateway/registry）
   crypto/secret ───────┘   （config::GanyuConfig）  治理面（baseline 自检 / 审计）
```

- **两层门控**：危险能力（exec/插件）需「特性编译 + env 放行」双保险（失败闭环）；
- **默认全关**：缓存/限速/审计/加密均为 0/关，显式开启；
- **基线自检**：启动打印高危组合告警（如 shell 开但无容器隔离），不阻断。

## 2. 全量环境变量

| 变量 | 默认 | 作用 |
|------|------|------|
| `GANYU_FS_ROOT` | `.ganyu_workspace` | 文件沙箱根（C3/C4） |
| `GANYU_ALLOW_SHELL` | 关 | `=1` 放行 exec（需 shell 特性） |
| `GANYU_ALLOW_PLUGINS` | 关 | `=1` 启用插件发现（C2） |
| `GANYU_PLUGIN_ALLOW` | 空（全拒） | 插件程序名白名单，逗号分隔 |
| `GANYU_MEM_KEY` | 无 | 记忆加密 passphrase（crypto 特性） |
| `GANYU_TOOL_CACHE_TTL` | `0`（关） | 只读工具结果缓存 TTL（毫秒） |
| `GANYU_LLM_CACHE_TTL` | `0`（关） | LLM 响应缓存 TTL（毫秒） |
| `GANYU_RATE_PER_MIN` | `0`（不限） | 网关每分钟请求上限 |
| `GANYU_AUDIT` | 关 | `1`/`stderr`/文件路径 → JSON Lines 审计 |
| `OV_BASE` | 无 | OpenViking 记忆服务（network） |
| `OPENAI_API_BASE` / `OPENAI_API_KEY` | 无 | OpenAI 兼容后端（network） |
| `OPENAI_MODEL` | `gpt-4o-mini` | 模型 id（OpenAI 兼容端点；推理模型自动兼容 `reasoning_content`） |

### 配置文件（一站式，对标 OpenClaw config.yaml）

写一次 `~/.ganyu/config.toml`，之后 `ganyu-agent chat` 直接对话，无需 export：

```toml
[model]
base_url = "https://apihub.agnes-ai.com/v1"
api_key = "sk-..."
model = "agnes-2.5-flash"
```

规则：路径优先级 `$GANYU_CONFIG` > `~/.ganyu/config.toml` > `./ganyu.toml`；
**已设置的环境变量优先于文件**（CI/容器友好）。实现：`config::load_model_config()`。

## 3. 场景配置模板

### A. 最小离线（默认构建，最安全）
```bash
cargo build
# 无需任何 env：无 exec、无插件、文件锁沙箱、明文记忆（无敏感数据）
ganyu-agent run "你好"
```

### B. 开发调试（真模型 + 审计 + 缓存）
```bash
cargo build --features network
export OPENAI_API_BASE=https://api.openai.com/v1 OPENAI_API_KEY=sk-...
export GANYU_TOOL_CACHE_TTL=30000 GANYU_LLM_CACHE_TTL=60000
export GANYU_AUDIT=1 GANYU_RATE_PER_MIN=120
```

### C. 生产加固（推荐组合）
```bash
cargo build --release --features hardened
export GANYU_MEM_KEY='<≥16 字符强口令>'
export GANYU_RATE_PER_MIN=60
export GANYU_TOOL_CACHE_TTL=30000 GANYU_LLM_CACHE_TTL=60000
export GANYU_AUDIT=/var/log/ganyu/audit.jsonl
# exec/插件保持默认关闭；确需时再开并加容器隔离
```

### D. 容器强隔离（生产最高级）
```bash
docker run --rm -it \
  -v /srv/ganyu-workspace:/workspace \
  -e GANYU_FS_ROOT=/workspace \
  -e GANYU_AUDIT=1 \
  -e GANYU_MEM_KEY=... \
  -v ganyu-data:/root/.ganyu \
  ganyu-agent:latest sag "上月华东区利润最高的三个产品"
```
> 容器内 exec 才建议开启：进程被容器边界兜住（对齐 Prime「进程隔离≠沙箱」的诚实边界——
> 沙箱根只是第一道防线，强隔离在容器/VM 层）。

## 4. 基线自检会提示什么

| 触发 | 建议 |
|------|------|
| `GANYU_ALLOW_SHELL=1` 但无 sandbox/容器 | 用 Docker/gVisor 或 Linux 加 `--features sandbox` |
| 插件开但 `GANYU_PLUGIN_ALLOW` 空 | 全部插件被拒（安全但无用），补白名单 |
| `GANYU_MEM_KEY` < 12 字符 | 加密强度不足，建议 ≥16 强口令 |
| LLM 缓存开但无限速 | 建议同时设 `GANYU_RATE_PER_MIN` |

## 5. 常见问题

- **为什么不是 config.toml？** 保持零依赖与离线优先；env 是单一来源（ADR-006 已记录，
  config.toml 文件化为后续迭代项，Pi 式「配置即文件」）。现已支持 `~/.ganyu/config.toml`（见 §2.5）。
- **密钥安全**：`GANYU_MEM_KEY`/`OPENAI_API_KEY` 勿写入仓库；生产从密钥管理器注入，
  `secret` 特性下 API key 内存清零（L1）。
- **改了 env 要重启吗？** 配置在启动时读取一次；网关后端可运行时 `hot_reload`（network）。
- **`web_fetch` 抓取报 Ssrf / 外网"不能联网"？** Clash 类代理用 fake-ip（198.18.0.0/15、
  fdfe:dcba:9876::/48）解析域名——SSRF 防护已豁免该网段（连接经代理转发）；若仍失败：
  确认代理在运行（`HTTPS_PROXY` env 或系统代理），`ganyu-agent doctor` 查看配置。
