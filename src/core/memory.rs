//! 抽象层之二：记忆 `Memory`。
//!
//! - `LocalMemory`：纯本地 JSON 持久化，零依赖，开箱即用。
//! - `OpenVikingMemory`：`LocalMemory` 的 drop-in 包装。记忆层自愈：本地存储作为安全网，
//!   OV_BASE 可达时本应代理到 :1933 REST，不可达时自动降级本地，不阻断 agent 运行。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::GanyuResult;
use crate::value::Value;

/// 一次检索命中。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub uri: String,
    pub score: f32,
    pub l0: String,
}

/// 记忆抽象：命名空间 URI（viking://...）键 → 统一字符串值。
#[async_trait]
pub trait Memory: Send + Sync {
    async fn put(&self, uri: &str, content: &Value) -> GanyuResult<()>;
    async fn get(&self, uri: &str) -> GanyuResult<Option<Value>>;
    async fn search(&self, query: &str, uri: &str) -> GanyuResult<Vec<MemoryHit>>;
    async fn commit(&self, trace: &Value) -> GanyuResult<()>;
}

/// 便于统一持有。
pub type DynMemory = std::sync::Arc<dyn Memory + Send + Sync>;

/// 本地记忆：扁平 `URI -> Value` 映射，落盘为 JSON。
pub struct LocalMemory {
    path: PathBuf,
    store: Mutex<HashMap<String, Value>>,
}

impl LocalMemory {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let store = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, Value>>(&s).ok())
            .unwrap_or_default();
        LocalMemory {
            path,
            store: Mutex::new(store),
        }
    }

    fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(&*self.store.lock().unwrap()) {
            let _ = std::fs::write(&self.path, s);
        }
    }
}

#[async_trait]
impl Memory for LocalMemory {
    async fn put(&self, uri: &str, content: &Value) -> GanyuResult<()> {
        self.store.lock().unwrap().insert(uri.to_string(), content.clone());
        self.save();
        Ok(())
    }

    async fn get(&self, uri: &str) -> GanyuResult<Option<Value>> {
        Ok(self.store.lock().unwrap().get(uri).cloned())
    }

    async fn search(&self, query: &str, uri: &str) -> GanyuResult<Vec<MemoryHit>> {
        let q = query.to_lowercase();
        let store = self.store.lock().unwrap();
        let mut hits = Vec::new();
        for (k, v) in store.iter() {
            let under_scope = uri == "viking://" || k.starts_with(uri);
            if !under_scope {
                continue;
            }
            let hay = format!("{k} {v}").to_lowercase();
            if hay.contains(&q) {
                let snippet: String = v.as_str().chars().take(80).collect();
                hits.push(MemoryHit {
                    uri: k.clone(),
                    score: 0.8,
                    l0: snippet,
                });
            }
        }
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        hits.truncate(5);
        Ok(hits)
    }

    async fn commit(&self, trace: &Value) -> GanyuResult<()> {
        let key = format!("viking://agent/memory/sessions/{}", crate::session::SessionId::new());
        self.store.lock().unwrap().insert(key, trace.clone());
        self.save();
        Ok(())
    }
}

/// 记忆层自愈包装：本地存储为安全网；OV_BASE 可达时可由子类代理到 OpenViking :1933。
pub struct OpenVikingMemory {
    inner: LocalMemory,
    #[allow(dead_code)]
    ov_base: Option<String>,
}

impl OpenVikingMemory {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let ov_base = std::env::var("OV_BASE").ok();
        OpenVikingMemory {
            inner: LocalMemory::new(path),
            ov_base,
        }
    }
}

#[async_trait]
impl Memory for OpenVikingMemory {
    async fn put(&self, uri: &str, content: &Value) -> GanyuResult<()> {
        self.inner.put(uri, content).await
    }
    async fn get(&self, uri: &str) -> GanyuResult<Option<Value>> {
        self.inner.get(uri).await
    }
    async fn search(&self, query: &str, uri: &str) -> GanyuResult<Vec<MemoryHit>> {
        self.inner.search(query, uri).await
    }
    async fn commit(&self, trace: &Value) -> GanyuResult<()> {
        self.inner.commit(trace).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_get_search() {
        let m = LocalMemory::new(".ganyu_test_mem.json");
        m.put("viking://agent/memory/cases/profit", &Value("华东利润Top3-成功".into()))
            .await
            .unwrap();
        let got = m.get("viking://agent/memory/cases/profit").await.unwrap();
        assert_eq!(got, Some(Value("华东利润Top3-成功".into())));
        let hits = m.search("利润", "viking://").await.unwrap();
        assert!(!hits.is_empty());
        let _ = std::fs::remove_file(".ganyu_test_mem.json");
    }
}
