//! Router 范式：分类器把请求派发给最匹配的 agent / 技能；无命中走 fallback。
//!
//! - `Router` trait：纯分类（`route` 返回路由键），可插拔（离线 `KeywordRouter` / 联网 LLM 路由器）。
//! - `RouterWorkflow`：路由表 `key -> Unit`；命中即用，未命中用 `fallback`。
//! 对齐"router-skill"：顶层先分类，再交给专精单元，避免单 agent 什么都做一点。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::core::unit::{RunContext, Unit};
use crate::error::GanyuResult;
use crate::value::Value;

use super::Workflow;

/// 路由分类器抽象：把输入映射到路由键（对应路由表中的一个单元）。
pub trait Router: Send + Sync {
    fn route(&self, input: &str) -> String;
}

/// 离线关键字路由器：命中规则返回键，否则返回空串（由 workflow 走 fallback）。
pub struct KeywordRouter {
    rules: Vec<(String, String)>,
}

impl KeywordRouter {
    pub fn new(rules: Vec<(&str, &str)>) -> Self {
        KeywordRouter {
            rules: rules
                .into_iter()
                .map(|(k, v)| (k.to_lowercase(), v.to_string()))
                .collect(),
        }
    }
}

impl Router for KeywordRouter {
    fn route(&self, input: &str) -> String {
        let q = input.to_lowercase();
        // 选择“最长匹配”的关键字，避免靠前的短关键字吞掉更具体的规则（F-14）。
        let mut best: Option<(usize, String)> = None;
        for (kw, key) in &self.rules {
            if q.contains(kw) {
                let score = kw.chars().count();
                if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
                    best = Some((score, key.clone()));
                }
            }
        }
        best.map(|b| b.1).unwrap_or_default()
    }
}

pub struct RouterWorkflow {
    router: Arc<dyn Router>,
    routes: HashMap<String, Arc<dyn Unit>>,
    fallback: Arc<dyn Unit>,
}

impl RouterWorkflow {
    pub fn new(
        router: Arc<dyn Router>,
        routes: HashMap<String, Arc<dyn Unit>>,
        fallback: Arc<dyn Unit>,
    ) -> Self {
        RouterWorkflow {
            router,
            routes,
            fallback,
        }
    }
}

#[async_trait]
impl Workflow for RouterWorkflow {
    fn mode(&self) -> &str {
        "router"
    }

    async fn run(&self, ctx: &RunContext, input: &Value) -> GanyuResult<Value> {
        let key = self.router.route(input.as_str());
        let unit = match self.routes.get(&key) {
            Some(u) => u,
            None => &self.fallback,
        };
        unit.run(ctx, input).await
    }
}
