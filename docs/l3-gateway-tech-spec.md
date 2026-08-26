# L3 生态兼容：多平台网关适配器（Tech-Spec）

> 状态：草稿（待用户确认范围后进入实现）
> 关联：L1 MCP 客户端（已落地）、L2 Pi 配置适配器（已落地）
> 范式对标：OpenClaw「多聊天平台网关」× Hermes「防护闭环」× Prime「诚实边界」

## 1. 背景与痛点

当前 `main.rs` 内联一个 **Telegram-only** 网关（`gateway start`，约 100 行裸 `reqwest` 调 Telegram Bot API）。
问题：
- 单平台，无抽象，加 Discord/Slack/Webhook 要再复制 100 行。
- 逻辑散在 `main.rs`，无单测，与 CLI 解析耦合。
- 不符合「OpenClaw 式多平台网关」的既定生态兼容路线图。

## 2. 目标

把聊天平台网关重构为 `src/gateway/` 模块下的 **`PlatformAdapter` trait**，支持多平台装配，fail-closed 默认只启用已配置 token 的平台。

## 3. 设计

### 3.1 核心抽象（`src/gateway/mod.rs`）

```rust
pub struct InboundMessage { pub chat_id: String, pub user: String, pub text: String }

pub trait PlatformAdapter: Send + Sync {
    fn name(&self) -> &str;                       // "telegram" / "http" / "discord"
    async fn poll(&self) -> GanyuResult<Vec<InboundMessage>>;
    async fn send(&self, chat: &str, text: &str) -> GanyuResult<()>;
    fn requires_network(&self) -> bool { true }
}

/// 按配置/特性装配，fail-closed：无 token/未启用的平台不加入。
pub fn build_adapters(cfg: &GanyuConfig) -> Vec<Box<dyn PlatformAdapter>>;
```

### 3.2 平台实现

- `src/gateway/telegram.rs`：搬入现有 inline 逻辑（`getMe`/`getUpdates` 长轮询/`sendMessage`），`#[cfg(feature="network")]`。
- 第二平台（范围待定，见 §5）。

### 3.3 `main.rs` 改造

- `gateway start` 改为：装配 adapters → 每个 adapter `tokio::spawn` 并发运行 → 每个 `chat_id` 懒创建独立 `Agent` session（保留现有会话隔离逻辑）。
- 无 adapter 时打印明确提示并退出（fail-closed）。

### 3.4 配置（`config.rs`）

`[gateway]` 段扩展（向后兼容现有 `telegram_token`）：
```toml
[gateway]
telegram_token = "..."      # 现有，保留
# http_bind = "127.0.0.1:8080"   # 第二平台可选
```

### 3.5 能力矩阵同步

`capability_matrix()` 补 `gateway:telegram` / `gateway:http` 条目（运行时 env/特性门控），`doctor`/`capabilities` 随之可见边界。

## 4. 安全边界（fail-closed）

- 无 token → 该平台不启用（不 panic、不泄露）。
- `network` 特性缺失 → 编译期排除平台代码，运行时提示。
- 每 chat 独立 session，消息不串上下文（保留现有隔离）。
- 模型输出经 `security::sanitize_model_output` 净化（复用 `routing::Gateway` 既有逻辑）。

## 5. 范围分叉（待用户确认）

| 选项 | 第二平台 | 新依赖 | 工作量 | 说明 |
|------|---------|--------|--------|------|
| A（推荐） | HTTP Webhook 桥接 | axum（或复用 reqwest 起最小 server） | 中 | 最贴近 OpenClaw 式：任意平台经 OpenClaw 转发到 ganyu 的 HTTP 端点，零平台 SDK |
| B | Discord 适配器 | serenity/twilight | 大 | 真实 Discord 网关，重依赖 |
| C | 仅重构 trait + Telegram 单平台 | 无 | 小 | 最小改动，留扩展点，第二平台后续再加 |

## 6. 验收标准

- `cargo build --features hardened` 编译通过（含新模块）。
- 单测：`build_adapters` 无 token 时返回空（fail-closed）；Telegram 单元（mock reqwest）通过。
- `doctor`/`capabilities` 正确显示 gateway 能力边界。
- CI 三平台全绿（打 `v0.1.16` tag 触发）。

## 7. 不在范围

- Prime RLM 推理范式（独立子方向，本次不做）。
- 真实多租户持久化 session（重启后为新 session，保留现状）。
