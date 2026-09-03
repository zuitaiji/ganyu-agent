# L3 生态兼容：多平台网关适配器（Tech-Spec）

> 状态：**已实现**（v0.1.16 落地，v0.1.17 加固鉴权与常驻可用性）
> 关联：L1 MCP 客户端（已落地）、L2 Pi 配置适配器（已落地）
> 范式对标：OpenClaw「多聊天平台网关」× Hermes「防护闭环」× Prime「诚实边界」

## 1. 背景与痛点

v0.1.16 之前 `main.rs` 内联一个 **Telegram-only** 网关（`gateway start`，约 100 行裸 `reqwest` 调 Bot API）：
- 单平台、无抽象，加 Discord/Slack/Webhook 要再复制 100 行；
- 逻辑散在 `main.rs`，无单测，与 CLI 解析耦合；
- 不符合「OpenClaw 式多平台网关」的既定生态兼容路线。

## 2. 目标

把聊天平台接入抽象为 `src/gateway/` 下的 **`PlatformAdapter` trait**，支持多平台装配；
fail-closed：仅启用已配置凭据的平台，且非回环暴露必须鉴权。

## 3. 设计

### 3.1 核心抽象（`src/gateway/mod.rs`）

```rust
pub struct InboundMessage { pub chat_id: String, pub user: String, pub text: String }

#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    fn name(&self) -> &str;                       // "telegram" / "http"
    async fn poll(&self) -> GanyuResult<Vec<InboundMessage>>;
    async fn send(&self, chat: &str, text: &str) -> GanyuResult<()>;
    fn requires_network(&self) -> bool { true }
}

pub async fn build_adapters(cfg: &GanyuConfig) -> Vec<Box<dyn PlatformAdapter>>;
pub async fn run_adapter(adapter: Box<dyn PlatformAdapter>, deps: GatewayDeps) -> GanyuResult<()>;
```

`#[async_trait]` 是 `Box<dyn PlatformAdapter>` 对象安全的前提（缺注解会触发 E0038 / E0195），
由单测 `trait_object_dispatch_works` 固化该不变量。

### 3.2 平台实现

| 适配器 | 文件 | 传输 | 说明 |
|--------|------|------|------|
| `TelegramAdapter` | `gateway/telegram.rs` | 裸 `reqwest` 调 Bot API | `getUpdates` 长轮询（timeout=25s）+ `sendMessage`，零 SDK |
| `HttpBridge` | `gateway/http_bridge.rs` | axum 本地端点 `POST /message` | 请求/响应桥接：入队 → 挂起 oneshot（120s）→ `send()` 回送 HTTP 响应 |
| `DiscordAdapter` | `gateway/discord.rs` | 裸 `reqwest` 调 Discord REST API | `GET /channels/{id}/messages?after=` 轮询 + `POST` 发送，鉴权 `Bot <token>`；不回放历史（首次置位游标），分页走最旧方向防 >100 漏消息，2s 空闲节流 |
| `SlackAdapter` | `gateway/slack.rs` | 裸 `reqwest` 调 Slack Web API | `GET /conversations.history?channel=&oldest=` 轮询 + `POST /chat.postMessage`，鉴权 `Bearer <token>`；不回放历史（首次置位游标），分页跟随 `next_cursor` 直至 `has_more=false`，显式判别 `ok:false`（Slack 错误返回 HTTP 200），跳过 `bot_id` 消息防自激循环，`chat_id` 由查询频道注入（响应体不含该字段），2s 空闲节流 |

四者均在 `#[cfg(feature = "network")]` 下编译。

### 3.3 `main.rs` 改造

- `gateway setup <bot_token>`：掩码写入 `[gateway] telegram_token`，文件权限收紧（属主只读）。
- `gateway start`：装配 adapters → 每个 adapter `tokio::spawn` 并发 → 每 `chat_id` 懒创建独立 `Agent` session。
- 无 adapter 时打印明确提示并退出（fail-closed）。

### 3.4 配置（`config.rs`）

`[gateway]` 段（向后兼容 `telegram_token`）：

```toml
[gateway]
telegram_token = "..."        # 可选：启用 Telegram
http_bind = "127.0.0.1:8080"  # 可选：启用 HTTP 桥接
http_token = "..."            # 非回环绑定时必填（见 §4.2）
```

等价环境变量：`GANYU_HTTP_BIND`、`GANYU_HTTP_TOKEN`；
L2 Pi `settings.json` 亦可叠加 `http_bind` / `http_token`。

### 3.5 能力矩阵同步

`capability_matrix()` 已补 `gateway:telegram` / `gateway:http (Webhook 桥接)` 两行（运行时凭据 + 特性门控判定），
`doctor` / `capabilities` 可见边界。

## 4. 安全边界（fail-closed）

### 4.1 通用

- 无凭据 → 该平台不启用（不 panic、不泄露）。
- 缺 `network` 特性 → 编译期排除平台代码，运行时提示。
- 每 chat 独立 session，消息不串上下文。
- 模型输出经 `security::sanitize_model_output` 净化。

### 4.2 HTTP 桥接鉴权（v0.1.17 加固）

桥接端点会驱动 agent——而 agent 在 `hardened` 构建下具备 shell / 文件 / MCP 工具权限。
无鉴权暴露等同于把本机执行权交给网络上可达的任何人。故：

| 规则 | 行为 |
|------|------|
| 绑定**非回环**地址且**未配置** token | **拒绝启动**（fail-closed），不降级为"先跑起来" |
| 配置了 token | 请求须带 `Authorization: Bearer <token>`，否则 `401` |
| 未配置 token（仅回环可达） | 放行，供本机 / 容器内桥接使用 |
| 请求体 | 上限 64 KiB，超限由 axum 直接返回 `413` |
| 空消息文本 | `400` |

非回环绑定还会在 `security_baseline()` 中产出告警，提示必须配置 token 并置于可信网络 / 反向代理之后。

## 5. 常驻可用性：`panic = "abort"` 下的并发模型

`profile.release` 设 `panic = "abort"`（源码无 `catch_unwind`，可安全 abort 以减小体积）。
但 `gateway start` 是**长驻进程**：任何一次 panic 都会 abort 整个进程，所有平台适配器一起下线。
因此网关内部**不允许出现可 panic 的锁或构造**：

- 队列 / 挂起表一律使用 `tokio::sync::Mutex`（异步锁，无 poisoning，`.lock().await`）；
- 不使用 `std::sync::Mutex` + `.unwrap()`——持锁线程 panic 后会 poison，下一次 `.unwrap()` 即 abort；
- `reqwest::Client` 构建失败降级为 `Client::new()`，不 `expect()`；
- `run_adapter` 常驻语义：单次 `poll` / `send` 失败只记录并退避重试，不让适配器退出。

## 6. 范围决策

第二平台选定 **选项 A：HTTP Webhook 桥接**（v0.1.16）。理由：零平台 SDK、依赖仅 axum（随 `network` 特性），
且任意平台只要能发 HTTP 即可经 OpenClaw 转发接入，最贴近既定网关路线。

首平台之后的**第三平台 `DiscordAdapter`（选项 B）已在 v0.1.21 采纳落地**：同样零 SDK、裸 `reqwest`
调 Discord REST，复用已验证的 `PlatformAdapter` 抽象与 `run_adapter` 驱动模型，无需新增依赖。
**第四平台 `SlackAdapter`（选项 B 的另一半）已在 v0.1.22 补齐**，同样零 SDK、无新依赖；
至此技术规格的选项 B（Discord/Slack）已全部落地。
"仅重构单平台"（选项 C）仍不采纳——抽象已就位，新增平台仅需实现 trait。

> 已知取舍：`SlackAdapter` 跳过带 `bot_id` 的消息，以阻断「回复自己 → 再被读回 → 再回复」
> 的自激循环（Slack 会把本 bot 发出的消息一并返回在 `conversations.history` 中）。
> `DiscordAdapter` 目前未做同类过滤，存在相同潜在面，留作后续一致性收口项。

## 7. 验收标准（已达成）

- `cargo build --release --features hardened --locked` 通过（CI 三平台）。
- 单测：
  - `trait_object_dispatch_works`（`dyn` 兼容性不变量）；
  - 鉴权：`no_token_allows_request` / `token_missing_or_wrong_is_rejected` / `valid_bearer_token_passes`；
  - fail-closed：`non_loopback_without_token_refuses_to_start`。
- `doctor` / `capabilities` 正确显示 gateway 能力边界。
- CI v0.1.16 三平台全绿、9 资产发布。

## 8. 不在范围

- Prime RLM 推理范式（独立子方向）。
- 真实多租户持久化 session（重启后为新 session，保留现状）。
- 桥接端点的 TLS 终结（交由前置反向代理负责）。
