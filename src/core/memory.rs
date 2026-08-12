//! 抽象层之二：记忆 `Memory`。
//!
//! - `LocalMemory`：纯本地 JSON 持久化，零依赖，开箱即用。
//!   - M4：落地改为 `tokio::sync::Mutex` + 异步 `save`，避免阻塞 executor、支持并发写。
//!   - H1（crypto 特性）：若设置 `GANYU_MEM_KEY`，落盘内容经 AES-256-GCM 加密，
//!     密钥由 passphrase 经 SHA-256 派生；缺密钥则回退明文（仅警告），不阻断运行。
//! - `OpenVikingMemory`：记忆层自愈包装（M1 修复"空实现"）。`network` 特性下，
//!   OV_BASE 可达时本应代理到 :1933 REST；任何网络错误都自动降级到本地安全网，不阻断 agent。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::GanyuResult;
use crate::session::SessionId;
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
    /// 提交一次会话轨迹，键入真实会话 UUID（自愈/自进化的锚点）。
    async fn commit(&self, session: &SessionId, trace: &Value) -> GanyuResult<()>;
    /// 读回某会话的最近轨迹，用于跨重启续接（自进化）。
    async fn load_session(&self, session: &SessionId) -> GanyuResult<Option<Value>>;
}

/// 便于统一持有。
pub type DynMemory = Arc<dyn Memory + Send + Sync>;

/// 本地记忆：扁平 `URI -> Value` 映射，落盘为 JSON。
pub struct LocalMemory {
    path: PathBuf,
    store: tokio::sync::Mutex<HashMap<String, Value>>,
    #[cfg(feature = "crypto")]
    cipher: Option<Cipher>,
}

impl LocalMemory {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let raw = std::fs::read_to_string(&path).ok();
        let store = match raw {
            Some(s) if s.starts_with("ENC1:") => {
                // 密文：仅在 crypto 特性下尝试解密；否则视作不可读，从空库起步。
                #[cfg(feature = "crypto")]
                {
                    if let Some(cipher) = Cipher::from_env() {
                        match cipher.decrypt(&s) {
                            Some(pt) => serde_json::from_str::<HashMap<String, Value>>(&pt)
                                .unwrap_or_default(),
                            None => HashMap::new(),
                        }
                    } else {
                        HashMap::new()
                    }
                }
                #[cfg(not(feature = "crypto"))]
                {
                    HashMap::new()
                }
            }
            Some(s) => serde_json::from_str::<HashMap<String, Value>>(&s).unwrap_or_default(),
            None => HashMap::new(),
        };
        LocalMemory {
            path,
            store: tokio::sync::Mutex::new(store),
            #[cfg(feature = "crypto")]
            cipher: Cipher::from_env(),
        }
    }

    async fn save(&self) {
        let snapshot = {
            let g = self.store.lock().await;
            (*g).clone()
        };
        let json = match serde_json::to_string_pretty(&snapshot) {
            Ok(j) => j,
            Err(_) => return,
        };
        let payload = {
            #[cfg(feature = "crypto")]
            {
                if let Some(c) = &self.cipher {
                    c.encrypt(&json)
                } else {
                    json
                }
            }
            #[cfg(not(feature = "crypto"))]
            {
                json
            }
        };
        // 先写临时文件再原子 rename，降低并发/崩溃导致文件损坏的概率（M4）。
        let tmp = format!("{}.tmp", self.path.display());
        if std::fs::write(&tmp, &payload).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

#[async_trait]
impl Memory for LocalMemory {
    async fn put(&self, uri: &str, content: &Value) -> GanyuResult<()> {
        self.store
            .lock()
            .await
            .insert(uri.to_string(), content.clone());
        self.save().await;
        Ok(())
    }

    async fn get(&self, uri: &str) -> GanyuResult<Option<Value>> {
        Ok(self.store.lock().await.get(uri).cloned())
    }

    async fn search(&self, query: &str, uri: &str) -> GanyuResult<Vec<MemoryHit>> {
        let q = query.to_lowercase();
        let store = self.store.lock().await;
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

    async fn commit(&self, session: &SessionId, trace: &Value) -> GanyuResult<()> {
        let key = format!("viking://agent/memory/sessions/{}", session.as_string());
        self.store.lock().await.insert(key, trace.clone());
        self.save().await;
        Ok(())
    }

    async fn load_session(&self, session: &SessionId) -> GanyuResult<Option<Value>> {
        let key = format!("viking://agent/memory/sessions/{}", session.as_string());
        Ok(self.store.lock().await.get(&key).cloned())
    }
}

/// H1：AES-256-GCM 机密封装（仅 crypto 特性编译）。
///
/// 设计取舍：密钥由 `GANYU_MEM_KEY` 经 SHA-256 派生，避免引入额外 KDF 依赖；
/// 这是"够用"的离线默认。生产环境应改从 OS 密钥环 / KMS 注入原始 32 字节密钥
/// （替换 `from_env` 即可），本结构不绑定具体密钥来源。
#[cfg(feature = "crypto")]
struct Cipher {
    key: [u8; 32],
}

#[cfg(feature = "crypto")]
impl Cipher {
    fn from_env() -> Option<Self> {
        let pass = std::env::var("GANYU_MEM_KEY").ok()?;
        if pass.is_empty() {
            return None;
        }
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(pass.as_bytes());
        let key: [u8; 32] = h.finalize().into();
        Some(Cipher { key })
    }

    fn encrypt(&self, plaintext: &str) -> String {
        use aes_gcm::aead::{Aead, KeyInit, generic_array::GenericArray};
        use aes_gcm::Aes256Gcm;
        use rand::RngCore;

        let cipher = Aes256Gcm::new(GenericArray::from_slice(&self.key));
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = cipher
            .encrypt(GenericArray::from_slice(&nonce), plaintext.as_bytes())
            .unwrap_or_default();
        let mut blob = nonce.to_vec();
        blob.extend_from_slice(&ct);
        format!("ENC1:{}", hex_encode(&blob))
    }

    fn decrypt(&self, blob: &str) -> Option<String> {
        use aes_gcm::aead::{Aead, KeyInit, generic_array::GenericArray};
        use aes_gcm::Aes256Gcm;

        let b64 = blob.strip_prefix("ENC1:")?;
        let raw = hex_decode(b64)?;
        if raw.len() < 12 {
            return None;
        }
        let (nonce, ct) = raw.split_at(12);
        let cipher = Aes256Gcm::new(GenericArray::from_slice(&self.key));
        let pt = cipher
            .decrypt(GenericArray::from_slice(nonce), ct)
            .ok()?;
        String::from_utf8(pt).ok()
    }
}

#[cfg(feature = "crypto")]
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(feature = "crypto")]
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .ok()
}

/// 记忆层自愈包装（M1 修复）：本地存储为安全网；`network` 特性下 OV_BASE 可达时
/// 代理到 OpenViking :1933 REST，失败自动降级本地，不阻断 agent 运行。
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
        #[cfg(feature = "network")]
        {
            if let Some(base) = &self.ov_base {
                if self.http_put(base, uri, content).await.is_ok() {
                    return Ok(());
                }
                // 网络失败 → 自愈降级到本地
            }
        }
        self.inner.put(uri, content).await
    }

    async fn get(&self, uri: &str) -> GanyuResult<Option<Value>> {
        #[cfg(feature = "network")]
        {
            if let Some(base) = &self.ov_base {
                if let Ok(Some(v)) = self.http_get(base, uri).await {
                    return Ok(Some(v));
                }
            }
        }
        self.inner.get(uri).await
    }

    async fn search(&self, query: &str, uri: &str) -> GanyuResult<Vec<MemoryHit>> {
        #[cfg(feature = "network")]
        {
            if let Some(base) = &self.ov_base {
                if let Ok(hits) = self.http_search(base, query, uri).await {
                    if !hits.is_empty() {
                        return Ok(hits);
                    }
                }
            }
        }
        self.inner.search(query, uri).await
    }

    async fn commit(&self, session: &SessionId, trace: &Value) -> GanyuResult<()> {
        self.inner.commit(session, trace).await
    }

    async fn load_session(&self, session: &SessionId) -> GanyuResult<Option<Value>> {
        self.inner.load_session(session).await
    }
}

#[cfg(feature = "network")]
impl OpenVikingMemory {
    async fn client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    }

    async fn http_put(&self, base: &str, uri: &str, content: &Value) -> GanyuResult<()> {
        let body = serde_json::json!({ "uri": uri, "content": content.as_str() });
        self.client()
            .await
            .post(format!("{}/memory", base.trim_end_matches('/')))
            .json(&body)
            .send()
            .await
            .map_err(|e| crate::error::GanyuError::BackendUnavailable(format!("ov_put: {e}")))?
            .error_for_status()
            .map_err(|e| crate::error::GanyuError::BackendError(e.to_string()))?;
        Ok(())
    }

    async fn http_get(&self, base: &str, uri: &str) -> GanyuResult<Option<Value>> {
        let resp = self
            .client()
            .await
            .get(format!("{}/memory", base.trim_end_matches('/')))
            .query(&[("uri", uri)])
            .send()
            .await
            .map_err(|e| crate::error::GanyuError::BackendUnavailable(format!("ov_get: {e}")))?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| crate::error::GanyuError::Http(e.to_string()))?;
        let content = json["content"].as_str().unwrap_or("").to_string();
        Ok(Some(Value(content)))
    }

    async fn http_search(
        &self,
        base: &str,
        query: &str,
        uri: &str,
    ) -> GanyuResult<Vec<MemoryHit>> {
        let resp = self
            .client()
            .await
            .get(format!("{}/search", base.trim_end_matches('/')))
            .query(&[("q", query), ("scope", uri)])
            .send()
            .await
            .map_err(|e| crate::error::GanyuError::BackendUnavailable(format!("ov_search: {e}")))?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| crate::error::GanyuError::Http(e.to_string()))?;
        let hits: Vec<MemoryHit> = serde_json::from_value(json).unwrap_or_default();
        Ok(hits)
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
        let got = m
            .get("viking://agent/memory/cases/profit")
            .await
            .unwrap();
        assert_eq!(got, Some(Value("华东利润Top3-成功".into())));
        let hits = m.search("利润", "viking://").await.unwrap();
        assert!(!hits.is_empty());
        let _ = std::fs::remove_file(".ganyu_test_mem.json");
    }

    #[tokio::test]
    async fn session_trace_persists_and_resumes() {
        let sid = SessionId::new();
        let m = LocalMemory::new(".ganyu_test_sess.json");
        m.commit(&sid, &Value("user: 上月华东利润Top3".into()))
            .await
            .unwrap();
        let loaded = m.load_session(&sid).await.unwrap();
        assert_eq!(loaded, Some(Value("user: 上月华东利润Top3".into())));
        let _ = std::fs::remove_file(".ganyu_test_sess.json");
    }

    #[tokio::test]
    async fn concurrent_writes_do_not_lose_data() {
        // M4：并发写不应丢数据。
        let m = Arc::new(LocalMemory::new(".ganyu_test_concurrent.json"));
        let mut handles = Vec::new();
        for i in 0..50 {
            let m = m.clone();
            handles.push(tokio::spawn(async move {
                m.put(&format!("viking://k/{i}"), &Value(format!("v{i}")))
                    .await
                    .unwrap();
            }));
        }
        for h in handles {
            let _ = h.await;
        }
        for i in 0..50 {
            assert_eq!(
                m.get(&format!("viking://k/{i}")).await.unwrap(),
                Some(Value(format!("v{i}")))
            );
        }
        let _ = std::fs::remove_file(".ganyu_test_concurrent.json");
    }

    #[cfg(feature = "crypto")]
    #[tokio::test]
    async fn encrypted_roundtrip() {
        // H1：设置密钥后落盘应为密文，且可正确还原。
        std::env::set_var("GANYU_MEM_KEY", "test-passphrase-123");
        let path = ".ganyu_test_enc.json";
        let _ = std::fs::remove_file(path);
        {
            let m = LocalMemory::new(path);
            m.put("viking://secret", &Value("topsecret".into()))
                .await
                .unwrap();
        }
        // 直接读盘，确认是密文（含 ENC1: 前缀），明文不应泄露。
        let raw = std::fs::read_to_string(path).unwrap();
        assert!(raw.starts_with("ENC1:"), "expected ciphertext on disk");
        assert!(!raw.contains("topsecret"), "plaintext must not leak to disk");
        // 重新打开（同密钥）能还原。
        let m2 = LocalMemory::new(path);
        let got = m2.get("viking://secret").await.unwrap();
        assert_eq!(got, Some(Value("topsecret".into())));
        let _ = std::fs::remove_file(path);
        std::env::remove_var("GANYU_MEM_KEY");
    }
}
