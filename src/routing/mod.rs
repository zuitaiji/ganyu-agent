//! 模型路由网关：单端点 + 多后端 + 自动选路（自愈核心）。
//!
//! 行为对齐规划中的 ai-router / OmniRoute 合成：
//! - 级联 fallback：后端依次尝试，首个成功即返回。
//! - lkgp 粘路径：记住上次成功的后端，下次优先。
//! - 熔断：不健康后端临时拉黑，冷却后允许探针。

use std::collections::HashMap;
use std::sync::Mutex;

use crate::core::llm::{DynBackend, Message};
use crate::error::{GanyuError, GanyuResult};
use crate::heal::CircuitBreaker;
use crate::value::Value;

pub struct Gateway {
    backends: Vec<DynBackend>,
    breakers: Mutex<HashMap<String, CircuitBreaker>>,
    last_good: Mutex<Option<String>>,
}

impl Gateway {
    pub fn new() -> Self {
        Gateway {
            backends: Vec::new(),
            breakers: Mutex::new(HashMap::new()),
            last_good: Mutex::new(None),
        }
    }

    pub fn register(&mut self, backend: DynBackend) {
        let name = backend.name().to_string();
        self.breakers
            .lock()
            .unwrap()
            .entry(name)
            .or_insert_with(|| CircuitBreaker::new(3, std::time::Duration::from_secs(5)));
        self.backends.push(backend);
    }

    /// lkgp：把上次成功的后端排到最前。
    fn ordered_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.backends.iter().map(|b| b.name().to_string()).collect();
        if let Some(lg) = self.last_good.lock().unwrap().clone() {
            if let Some(pos) = names.iter().position(|n| *n == lg) {
                let n = names.remove(pos);
                names.insert(0, n);
            }
        }
        names
    }

    /// 经网关完成一次补全：级联 + 熔断 + lkgp，全失败则自愈失败。
    pub async fn complete(&self, messages: &[Message]) -> GanyuResult<Value> {
        let names = self.ordered_names();
        let mut last_err: Option<GanyuError> = None;
        for name in names {
            let backend = self
                .backends
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
                    return Ok(out);
                }
                Err(e) => {
                    self.breakers
                        .lock()
                        .unwrap()
                        .get_mut(&name)
                        .unwrap()
                        .record_failure();
                    last_err = Some(e);
                }
            }
        }
        Err(GanyuError::AllBackendsFailed(format!("{last_err:?}")))
    }
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
        let mut g = Gateway::new();
        g.register(Arc::new(FailBackend));
        g.register(Arc::new(OkBackend));
        let out = g.complete(&[Message::user("hi")]).await.unwrap();
        assert_eq!(out, Value("ok".into()));
        // 第二次应粘到 ok（lkgp）
        let out2 = g.complete(&[Message::user("hi")]).await.unwrap();
        assert_eq!(out2, Value("ok".into()));
    }
}
