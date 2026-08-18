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
    /// 加密记忆无法解密（密钥缺失/错误）时置位：`save` 将跳过，
    /// 保护原密文文件不被空库静默覆盖（密钥输错 ≠ 记忆清零）。
    load_failed: bool,
}

impl LocalMemory {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let raw = std::fs::read_to_string(&path).ok();
        let mut load_failed = false;
        let store = match raw {
            Some(s) if s.starts_with("ENC1:") || s.starts_with("ENC2:") => {
                // 密文：仅在 crypto 特性下尝试解密；否则视作不可读，从空库起步。
                #[cfg(feature = "crypto")]
                {
                    if let Some(cipher) = Cipher::from_env() {
                        match cipher.decrypt(&s) {
                            Some(pt) => serde_json::from_str::<HashMap<String, Value>>(&pt)
                                .unwrap_or_default(),
                            None => {
                                // 密钥错误：不覆盖原文件（P2）
                                load_failed = true;
                                HashMap::new()
                            }
                        }
                    } else {
                        // 密文存在但无 GANYU_MEM_KEY：保护原文件（P2）
                        load_failed = true;
                        HashMap::new()
                    }
                }
                #[cfg(not(feature = "crypto"))]
                {
                    load_failed = true;
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
            load_failed,
        }
    }

    async fn save(&self) {
        // 加密记忆无法解密时绝不覆盖原文件（P2：密钥输错 ≠ 记忆清零）
        if self.load_failed {
            return;
        }
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
            // R-9：加密记忆文件含敏感 blob，写后收紧为仅属主可读写
            // （Unix 0600 / Windows 等价 ACL），再原子 rename。
            let _ = crate::security::restrict_file_permissions(&tmp);
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
/// 密钥派生（R-2 加固）：
/// - 口令 `GANYU_MEM_KEY` 先经 `stretch()` 迭代 SHA-256 拉伸（100k 轮）得到**主密钥**，
///   显著抬高"记忆文件被盗 + 弱口令"下的离线暴破成本；
/// - 每个记忆文件再用**随机盐**派生独立文件密钥 `SHA-256(master || salt)`，
///   杜绝"同口令 → 同密钥"；
/// - 落盘格式 `ENC2:<hex>`，hex = 盐(16) ‖ nonce(12) ‖ 密文；
/// - 仍兼容旧格式 `ENC1:`（无盐、单次 SHA-256 派生），旧记忆文件可正常解密。
///
/// 生产环境应改从 OS 密钥环 / KMS 注入原始 32 字节密钥（替换 `from_env` 即可），
/// 本结构不绑定具体密钥来源。
#[cfg(feature = "crypto")]
struct Cipher {
    /// 原始口令（仅进程内存使用）。
    pass: String,
    /// 口令经拉伸后的主密钥（无盐，一次性成本）；每文件再加随机盐派生文件密钥。
    master: [u8; 32],
}

/// KDF 迭代轮数：把 SHA-256 口令拉伸 100_000 轮，显著抬高离线暴破成本（R-2）。
const KDF_ROUNDS: u32 = 100_000;

#[cfg(feature = "crypto")]
fn sha256_sum(b: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b);
    h.finalize().into()
}

/// 口令拉伸（无盐，一次性）：迭代 SHA-256。盐在每文件级别引入（见 `file_key`）。
#[cfg(feature = "crypto")]
fn stretch(pass: &str) -> [u8; 32] {
    let mut key = sha256_sum(pass.as_bytes());
    for _ in 0..KDF_ROUNDS {
        key = sha256_sum(&key);
    }
    key
}

#[cfg(feature = "crypto")]
impl Cipher {
    fn from_env() -> Option<Self> {
        let pass = std::env::var("GANYU_MEM_KEY").ok()?;
        if pass.is_empty() {
            return None;
        }
        let master = stretch(&pass);
        Some(Cipher { pass, master })
    }

    /// 每文件密钥 = SHA-256(master ‖ salt)，使每个记忆文件使用独立密钥（R-2）。
    fn file_key(&self, salt: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(self.master);
        h.update(salt);
        h.finalize().into()
    }

    /// 旧格式（ENC1）兼容：原密钥 = 单次 SHA-256(passphrase)。
    fn legacy_key(&self) -> [u8; 32] {
        sha256_sum(self.pass.as_bytes())
    }

    fn encrypt(&self, plaintext: &str) -> String {
        use aes_gcm::aead::{Aead, KeyInit, generic_array::GenericArray};
        use aes_gcm::Aes256Gcm;
        use rand::RngCore;

        // 每文件随机盐，杜绝"同口令 → 同密钥"。
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        let fk = self.file_key(&salt);

        let cipher = Aes256Gcm::new(GenericArray::from_slice(&fk));
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = cipher
            .encrypt(GenericArray::from_slice(&nonce), plaintext.as_bytes())
            .unwrap_or_default();
        let mut blob = salt.to_vec();
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ct);
        format!("ENC2:{}", hex_encode(&blob))
    }

    fn decrypt(&self, blob: &str) -> Option<String> {
        if let Some(b64) = blob.strip_prefix("ENC2:") {
            let raw = hex_decode(b64)?;
            if raw.len() < 28 {
                return None;
            } // 16 盐 + 12 nonce
            let (salt, rest) = raw.split_at(16);
            let (nonce, ct) = rest.split_at(12);
            return aes_gcm_decrypt(&self.file_key(salt), nonce, ct);
        }
        if let Some(b64) = blob.strip_prefix("ENC1:") {
            // 向后兼容旧格式（无盐，单次 SHA-256 派生密钥）。
            let raw = hex_decode(b64)?;
            if raw.len() < 12 {
                return None;
            }
            let (nonce, ct) = raw.split_at(12);
            return aes_gcm_decrypt(&self.legacy_key(), nonce, ct);
        }
        None
    }
}

#[cfg(feature = "crypto")]
fn aes_gcm_decrypt(key: &[u8; 32], nonce: &[u8], ct: &[u8]) -> Option<String> {
    use aes_gcm::aead::{Aead, KeyInit, generic_array::GenericArray};
    use aes_gcm::Aes256Gcm;
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let pt = cipher.decrypt(GenericArray::from_slice(nonce), ct).ok()?;
    String::from_utf8(pt).ok()
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

    /// 加密测试共享 GANYU_MEM_KEY（进程全局 env），必须串行防竞态。
    #[cfg(feature = "crypto")]
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(feature = "crypto")]
    #[tokio::test]
    async fn encrypted_roundtrip() {
        let _g = ENV_LOCK.lock().unwrap();
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
        assert!(raw.starts_with("ENC2:"), "expected ciphertext (ENC2) on disk");
        assert!(!raw.contains("topsecret"), "plaintext must not leak to disk");
        // 重新打开（同密钥）能还原。
        let m2 = LocalMemory::new(path);
        let got = m2.get("viking://secret").await.unwrap();
        assert_eq!(got, Some(Value("topsecret".into())));
        let _ = std::fs::remove_file(path);
        std::env::remove_var("GANYU_MEM_KEY");
    }

    #[cfg(feature = "crypto")]
    #[tokio::test]
    async fn wrong_key_never_overwrites_encrypted_file() {
        let _g = ENV_LOCK.lock().unwrap();
        // P2：密钥错误时 put 不得把加密记忆库静默覆盖为空库（防永久丢失）。
        let path = ".ganyu_test_wrongkey.json";
        let _ = std::fs::remove_file(path);
        std::env::set_var("GANYU_MEM_KEY", "key-a");
        {
            let m = LocalMemory::new(path);
            m.put("viking://user/memory/x", &Value("secret".into()))
                .await
                .unwrap();
        }
        let encrypted = std::fs::read_to_string(path).unwrap();
        assert!(encrypted.starts_with("ENC2:"));
        // 换错误密钥：读取失败 → load_failed → put 不落盘
        std::env::set_var("GANYU_MEM_KEY", "key-b");
        {
            let m2 = LocalMemory::new(path);
            m2.put("viking://user/memory/y", &Value("other".into()))
                .await
                .unwrap();
        }
        let after = std::fs::read_to_string(path).unwrap();
        assert_eq!(encrypted, after, "密钥错误时不得覆盖加密记忆文件");
        // 恢复正确密钥仍可还原原数据
        std::env::set_var("GANYU_MEM_KEY", "key-a");
        let m3 = LocalMemory::new(path);
        let got = m3.get("viking://user/memory/x").await.unwrap();
        assert_eq!(got, Some(Value("secret".into())));
        let _ = std::fs::remove_file(path);
        std::env::remove_var("GANYU_MEM_KEY");
    }

    #[cfg(feature = "crypto")]
    #[tokio::test]
    async fn enc1_backward_compat_readable() {
        let _g = ENV_LOCK.lock().unwrap();
        // R-2 回归：旧格式 ENC1（无盐、单次 SHA-256 派生密钥）必须仍可被解密，
        // 否则旧记忆文件在升级后会变成"密钥正确也读不出"。
        std::env::set_var("GANYU_MEM_KEY", "key-a");
        let path = ".ganyu_test_enc1compat.json";
        let _ = std::fs::remove_file(path);

        // 用旧方案手动构造一条 ENC1 密文。
        use aes_gcm::aead::{Aead, KeyInit, generic_array::GenericArray};
        use aes_gcm::Aes256Gcm;
        use rand::RngCore;
        use sha2::{Digest, Sha256};
        let key: [u8; 32] = Sha256::new().chain_update(b"key-a").finalize().into();
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = Aes256Gcm::new(GenericArray::from_slice(&key))
            .encrypt(GenericArray::from_slice(&nonce), &b"legacy-topsecret"[..])
            .unwrap();
        let mut blob = nonce.to_vec();
        blob.extend_from_slice(&ct);
        let enc1 = format!("ENC1:{}", hex_encode(&blob));
        std::fs::write(path, &enc1).unwrap();

        // 明文不得落盘泄露。
        let raw = std::fs::read_to_string(path).unwrap();
        assert!(!raw.contains("legacy-topsecret"));
        // 同口令下 Cipher 能正确解码旧格式。
        let cipher = Cipher::from_env().expect("Cipher 应可用");
        let pt = cipher.decrypt(&enc1).expect("ENC1 必须可解密");
        assert_eq!(pt, "legacy-topsecret");

        let _ = std::fs::remove_file(path);
        std::env::remove_var("GANYU_MEM_KEY");
    }
}
