//! 模型路由网关：单端点 + 多后端 + 自动选路（自愈核心）。
//!
//! 行为对齐规划中的 ai-router / OmniRoute 合成：
//! - 级联 fallback：后端依次尝试，首个成功即返回。
//! - lkgp 粘路径：记住上次成功的后端，下次优先。
//! - 熔断：不健康后端临时拉黑，冷却后允许探针。
//! - M2：可选速率限制（令牌桶），防止对后端洪泛。
//! - L2：支持运行时热更新后端列表（`hot_reload`）。
//! - M5：所有模型输出在出口处经 `sanitize_model_output` 校验（长度上限 + 去 NUL）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::cache::{cache_key, LruCache};
use crate::core::llm::{DynBackend, Message};
use crate::error::{GanyuError, GanyuResult};
use crate::heal::{CircuitBreaker, RateLimiter};
use crate::observe::{AuditEvent, AuditLog};
use crate::security;
use crate::value::Value;

pub struct Gateway {
    backends: Mutex<Vec<DynBackend>>,
    breakers: Mutex<HashMap<String, CircuitBreaker>>,
    last_good: Mutex<Option<String>>,
    /// M2：可选速率限制器；`None` 表示不限速。
    rate: Mutex<Option<RateLimiter>>,
    /// LLM 响应缓存（LRU+TTL；默认 None=关）。相同 messages 序列在 TTL 内直接命中。
    llm_cache: Mutex<Option<LruCache<u64, Value>>>,
    /// 审计日志（默认 None=关）。
    audit: Mutex<Option<Arc<AuditLog>>>,
}

impl Gateway {
    pub fn new() -> Self {
        Gateway {
            backends: Mutex::new(Vec::new()),
            breakers: Mutex::new(HashMap::new()),
            last_good: Mutex::new(None),
            rate: Mutex::new(None),
            llm_cache: Mutex::new(None),
            audit: Mutex::new(None),
        }
    }

    /// M2：开启速率限制（每分钟上限）。
    pub fn with_rate_limit(self, per_min: u32) -> Self {
        *self.rate.lock().unwrap() = Some(RateLimiter::new(per_min));
        self
    }

    /// 开启 LLM 响应缓存（相同请求在 TTL 内命中，省模型调用）。`ttl` 为 0 视为关闭。
    pub fn enable_llm_cache(&self, ttl: Duration) {
        if ttl > Duration::ZERO {
            *self.llm_cache.lock().unwrap() = Some(LruCache::new(128, ttl));
        }
    }

    /// 挂接审计日志。
    pub fn set_audit(&self, log: Arc<AuditLog>) {
        *self.audit.lock().unwrap() = Some(log);
    }

    fn audit_evt(&self, ev: AuditEvent) {
        if let Some(a) = self.audit.lock().unwrap().as_ref() {
            a.event(ev);
        }
    }

    /// 注册后端（可运行时调用，支持热更新）。
    pub fn register(&self, backend: DynBackend) {
        let name = backend.name().to_string();
        self.breakers
            .lock()
            .unwrap()
            .entry(name)
            .or_insert_with(|| CircuitBreaker::new(3, Duration::from_secs(5)));
        self.backends.lock().unwrap().push(backend);
    }

    /// lkgp：把上次成功的后端排到最前；本地兜底（local）永远排最后（真模型优先）。
    fn ordered_names(&self) -> Vec<String> {
        let backends = self.backends.lock().unwrap();
        let mut remote: Vec<String> = Vec::new();
        let mut local: Vec<String> = Vec::new();
        for b in backends.iter() {
            if b.name() == "local" {
                local.push(b.name().to_string());
            } else {
                remote.push(b.name().to_string());
            }
        }
        drop(backends);
        if let Some(lg) = self.last_good.lock().unwrap().clone() {
            if lg != "local" {
                if let Some(pos) = remote.iter().position(|n| *n == lg) {
                    let n = remote.remove(pos);
                    remote.insert(0, n);
                }
            }
        }
        remote.extend(local);
        remote
    }

    /// 经网关完成一次补全：级联 + 熔断 + lkgp + 限速 + 缓存 + 输出净化，全失败则自愈失败。
    pub async fn complete(&self, messages: &[Message]) -> GanyuResult<Value> {
        // M2：速率限制
        if let Some(rl) = self.rate.lock().unwrap().as_ref() {
            if !rl.try_acquire() {
                self.audit_evt(AuditEvent::RateLimited { reason: "网关请求超过速率上限" });
                return Err(GanyuError::RateLimited("网关请求超过速率上限".into()));
            }
        }

        // LLM 响应缓存：相同 messages 序列直接命中（省模型调用）。
        let cache_on = self.llm_cache.lock().unwrap().is_some();
        let cache_key_val = if cache_on {
            let json = serde_json::to_string(messages).unwrap_or_default();
            Some(cache_key(&[&json]))
        } else {
            None
        };
        if let Some(k) = &cache_key_val {
            if let Some(hit) = self.llm_cache.lock().unwrap().as_ref().unwrap().get(k) {
                self.audit_evt(AuditEvent::LlmCacheHit { ms: 0 });
                return Ok(hit);
            }
        }

        let names = self.ordered_names();
        let backends = self.backends.lock().unwrap().clone();
        let mut last_err: Option<GanyuError> = None;
        let mut attempted: Vec<String> = Vec::new();
        for name in names {
            let backend = backends
                .iter()
                .find(|b| b.name() == name)
                .cloned()
                .unwrap();
            {
                let br = self.breakers.lock().unwrap();
                if !br.get(&name).unwrap().allow() {
                    continue; // 熔断中，跳过
                }
            }
            match backend.complete(messages).await {
                Ok(out) => {
                    self.breakers
                        .lock()
                        .unwrap()
                        .get_mut(&name)
                        .unwrap()
                        .record_success();
                    *self.last_good.lock().unwrap() = Some(name);
                    // M5：模型输出在信任边界处净化（长度上限 + 去 NUL）。
                    let clean = security::sanitize_model_output(out.as_str())?;
                    let final_val = Value(clean);
                    if let Some(k) = &cache_key_val {
                        self.llm_cache
                            .lock()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .put(*k, final_val.clone());
                    }
                    return Ok(final_val);
                }
                Err(e) => {
                    self.breakers
                        .lock()
                        .unwrap()
                        .get_mut(&name)
                        .unwrap()
                        .record_failure();
                    if let Some(prev) = attempted.last() {
                        self.audit_evt(AuditEvent::GatewayFallback { from: prev, to: &name });
                    }
                    attempted.push(name);
                    last_err = Some(e);
                }
            }
        }
        Err(GanyuError::AllBackendsFailed(format!("{last_err:?}")))
    }

    /// L2：从配置文件热更新后端列表（仅 `network` 特性下支持 OpenAI 类后端）。
    /// 配置格式：`[{"name","base_url","api_key","model"}]`。成功返回新注册的后端数。
    #[cfg(feature = "network")]
    pub fn hot_reload(&self, path: &str) -> GanyuResult<usize> {
        let raw = std::fs::read_to_string(path)?;
        let specs: Vec<BackendSpec> = serde_json::from_str(&raw)?;
        let mut new_backends: Vec<DynBackend> = Vec::new();
        for s in specs {
            new_backends.push(std::sync::Arc::new(crate::core::llm::OpenAiBackend::new(
                &s.base_url,
                &s.api_key,
                &s.model,
            )) as DynBackend);
        }
        // 原子替换后端表，并重置熔断器/粘路径，避免陈旧状态。
        *self.backends.lock().unwrap() = new_backends;
        *self.breakers.lock().unwrap() = HashMap::new();
        *self.last_good.lock().unwrap() = None;
        for b in self.backends.lock().unwrap().iter() {
            self.breakers
                .lock()
                .unwrap()
                .entry(b.name().to_string())
                .or_insert_with(|| CircuitBreaker::new(3, std::time::Duration::from_secs(5)));
        }
        Ok(self.backends.lock().unwrap().len())
    }
}

#[cfg(feature = "network")]
#[derive(serde::Deserialize)]
struct BackendSpec {
    #[allow(dead_code)]
    name: String,
    base_url: String,
    api_key: String,
    model: String,
}

impl Default for Gateway {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::llm::LlmBackend;
    use crate::error::GanyuResult;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct FailBackend;
    #[async_trait]
    impl LlmBackend for FailBackend {
        fn name(&self) -> &str { "fail" }
        async fn complete(&self, _: &[Message]) -> GanyuResult<Value> {
            Err(GanyuError::BackendUnavailable("fail".into()))
        }
    }
    struct OkBackend;
    #[async_trait]
    impl LlmBackend for OkBackend {
        fn name(&self) -> &str { "ok" }
        async fn complete(&self, _: &[Message]) -> GanyuResult<Value> {
            Ok(Value("ok".into()))
        }
    }

    #[tokio::test]
    async fn cascade_falls_through_to_ok() {
        let g = Gateway::new();
        g.register(Arc::new(FailBackend));
        g.register(Arc::new(OkBackend));
        let out = g.complete(&[Message::user("hi")]).await.unwrap();
        assert_eq!(out, Value("ok".into()));
        // 第二次应粘到 ok（lkgp）
        let out2 = g.complete(&[Message::user("hi")]).await.unwrap();
        assert_eq!(out2, Value("ok".into()));
    }

    #[tokio::test]
    async fn rate_limit_blocks_after_capacity() {
        let g = Gateway::new().with_rate_limit(2);
        // 同一后端连续 3 次：前 2 次成功，第 3 次在窗口内被限流。
        let ok = Arc::new(OkBackend);
        g.register(ok.clone());
        assert!(g.complete(&[Message::user("1")]).await.is_ok());
        assert!(g.complete(&[Message::user("2")]).await.is_ok());
        // 桶空了，下一次应被限流（BackendUnavailable 以外的错误类型）
        let r = g.complete(&[Message::user("3")]).await;
        assert!(matches!(r, Err(GanyuError::RateLimited(_))));
    }

    struct CountingBackend {
        n: Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait]
    impl LlmBackend for CountingBackend {
        fn name(&self) -> &str { "count" }
        async fn complete(&self, _: &[Message]) -> GanyuResult<Value> {
            self.n
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Value("ok".into()))
        }
    }

    #[tokio::test]
    async fn llm_cache_absorbs_repeat_calls() {
        let g = Gateway::new();
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        g.register(Arc::new(CountingBackend { n: n.clone() }));
        g.enable_llm_cache(Duration::from_secs(60));
        let r1 = g.complete(&[Message::user("hi")]).await.unwrap();
        let r2 = g.complete(&[Message::user("hi")]).await.unwrap();
        assert_eq!(r1, r2);
        assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 1, "第二次应命中 LLM 缓存");
    }
}
