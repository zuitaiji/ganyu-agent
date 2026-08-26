//! 多平台网关适配器（L3 生态兼容）：OpenClaw 式「多聊天平台网关」。
//!
//! 把聊天平台接入抽象为 `PlatformAdapter` trait，Telegram 与 HTTP 桥接是两个实现。
//! `build_adapters` 按配置/特性装配，fail-closed：仅启用已配置 token / 绑定地址的平台。
//!
//! 设计对齐 OpenClaw「多聊天平台网关」：任意平台（Telegram / Discord / Slack / 自研桥接）
//! 只要实现 `PlatformAdapter`，即可被 `run_adapter` 统一驱动——每 chat 独立 Agent 会话。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::core::Agent;
use crate::core::loop_::Reasoner;
use crate::core::memory::DynMemory;
use crate::ext::SkillBook;
use crate::ext::ToolRegistry;
use crate::routing::Gateway;
use crate::session::SessionId;
use crate::Value;
use crate::GanyuResult;

/// 入站消息（平台无关）。
pub struct InboundMessage {
    pub chat_id: String,
    pub user: String,
    pub text: String,
}

/// 平台适配器：每个聊天平台实现 `poll`（拉取新消息）与 `send`（回复）。
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// 平台标识（telegram / http / ...）。
    fn name(&self) -> &str;
    /// 拉取自上次调用以来的新消息（无消息返回空；出错返回空并在内部退避重试）。
    async fn poll(&self) -> GanyuResult<Vec<InboundMessage>>;
    /// 向指定 chat 回复文本。
    async fn send(&self, chat: &str, text: &str) -> GanyuResult<()>;
    /// 是否需要网络（默认 true；用于能力矩阵标注）。
    fn requires_network(&self) -> bool {
        true
    }
}

/// 共享依赖：构造每 chat 独立 `Agent` 所需（全部 `Arc`，可廉价 clone 给多适配器并发）。
#[derive(Clone)]
pub struct GatewayDeps {
    pub gateway: Arc<Gateway>,
    pub memory: DynMemory,
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillBook>,
    pub reasoner: Arc<dyn Reasoner>,
}

/// 运行单个适配器：拉取消息 → 每 chat 独立 Agent 推理 → 回复。
///
/// 会话隔离：相同 `chat_id` 复用同一 `Agent`/session；不同 chat 互不串上下文。
/// 任一 adapter 内部为长循环（Telegram 长轮询 / HTTP 桥接挂起队列），由调用方 `tokio::spawn` 并发。
pub async fn run_adapter(
    adapter: Box<dyn PlatformAdapter>,
    deps: GatewayDeps,
) -> GanyuResult<()> {
    let name = adapter.name().to_string();
    let mut chat_agents: HashMap<String, Arc<Agent>> = HashMap::new();
    println!("[gateway:{name}] 启动（每 chat 独立会话）");
    loop {
        let msgs = adapter.poll().await?;
        for m in msgs {
            println!("[gateway:{name}] {}: {}", m.user, m.text);
            let chat_agent = chat_agents.entry(m.chat_id.clone()).or_insert_with(|| {
                let sid = SessionId::new();
                println!("[gateway:{name}] 新会话 chat={} session={sid}", m.chat_id);
                Arc::new(Agent::new(
                    deps.gateway.clone(),
                    deps.memory.clone(),
                    deps.tools.clone(),
                    deps.skills.clone(),
                    deps.reasoner.clone(),
                    sid,
                ))
            });
            let out = match chat_agent.run(&Value(m.text.clone())).await {
                Ok(v) => v.to_string(),
                Err(e) => format!("抱歉，处理出错: {e}"),
            };
            println!("[gateway:{name}] ganyu: {out}");
            adapter.send(&m.chat_id, &out).await?;
        }
    }
}

#[cfg(feature = "network")]
mod telegram;
#[cfg(feature = "network")]
mod http_bridge;

#[cfg(feature = "network")]
pub use telegram::TelegramAdapter;
#[cfg(feature = "network")]
pub use http_bridge::HttpBridge;

/// 按配置/特性装配已启用的平台适配器（fail-closed：无 token / 绑定则不启用任何平台）。
#[cfg(feature = "network")]
pub async fn build_adapters(cfg: &crate::config::GanyuConfig) -> Vec<Box<dyn PlatformAdapter>> {
    let mut adapters: Vec<Box<dyn PlatformAdapter>> = Vec::new();
    if let Some(token) = crate::config::read_gateway_token() {
        adapters.push(Box::new(TelegramAdapter::new(&token)));
        println!("[gateway] 已装配 Telegram 适配器");
    }
    if let Some(bind) = &cfg.http_bind {
        match HttpBridge::new(bind).await {
            Ok(b) => {
                adapters.push(Box::new(b));
                println!("[gateway] 已装配 HTTP 桥接适配器（绑定 {bind}）");
            }
            Err(e) => eprintln!("[gateway] HTTP 桥接启动失败（{e}），跳过"),
        }
    }
    adapters
}

/// 非 network 构建：网关不可用，返回空（fail-closed，编译期排除平台代码）。
#[cfg(not(feature = "network"))]
pub async fn build_adapters(_cfg: &crate::config::GanyuConfig) -> Vec<Box<dyn PlatformAdapter>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 证明 `PlatformAdapter` 可作为 `dyn` 对象（异步 trait 的 dyn 兼容性，曾因缺
    /// `#[async_trait]` 导致 E0038；此测试固化该不变量）。
    struct EchoAdapter;
    #[async_trait]
    impl PlatformAdapter for EchoAdapter {
        fn name(&self) -> &str {
            "echo"
        }
        async fn poll(&self) -> GanyuResult<Vec<InboundMessage>> {
            Ok(vec![InboundMessage {
                chat_id: "c1".into(),
                user: "u".into(),
                text: "hi".into(),
            }])
        }
        async fn send(&self, _chat: &str, _text: &str) -> GanyuResult<()> {
            Ok(())
        }
    }

    #[test]
    fn trait_object_dispatch_works() {
        let a: Box<dyn PlatformAdapter> = Box::new(EchoAdapter);
        assert_eq!(a.name(), "echo");
        assert!(a.requires_network());
    }
}
