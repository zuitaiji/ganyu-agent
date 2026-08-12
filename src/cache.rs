//! 缓存优化层：LRU + TTL 通用缓存（M 系优化之一）。
//!
//! 对标 OpenClaw「热路径缓存复用」（install records / config JSON / tool search catalogs /
//! session stores）与 Hermes 的上下文压缩思想，ganyu 在两条热路径上做缓存：
//!
//! 1. **只读工具结果缓存**（`ToolRegistry`）：`calc`/`echo`/`file_read` 等幂等工具的
//!    结果可短暂缓存，避免重复计算/重复读盘。
//! 2. **LLM 响应缓存**（`Gateway`）：相同 `messages` 序列在 TTL 内直接命中，省一次模型调用。
//!
//! 安全约束（延续失败闭环哲学）：
//! - 缓存**默认关闭**，仅通过环境变量显式开启（`GANYU_TOOL_CACHE_TTL` / `GANYU_LLM_CACHE_TTL`）；
//! - **副作用工具永不缓存**（`side_effecting()==true` 直接跳过），防止陈旧状态复现；
//! - TTL 必须 >0 才生效，避免「零 TTL=永久缓存」的隐藏风险。

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 通用 LRU + TTL 缓存。
///
/// - `cap`：容量上限，超出时淘汰最久未使用项。
/// - `ttl`：条目存活时间；过期条目在 `get` 时惰性淘汰。
/// - 内部用 `HashMap` + 插入序向量实现 LRU（简单可靠，容量通常很小）。
pub struct LruCache<K, V> {
    map: Mutex<HashMap<K, (V, Instant)>>,
    order: Mutex<Vec<K>>,
    cap: usize,
    ttl: Duration,
}

impl<K, V> LruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(cap: usize, ttl: Duration) -> Self {
        LruCache {
            map: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
            cap: cap.max(1),
            ttl,
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let now = Instant::now();
        let mut map = self.map.lock().unwrap();
        match map.get(key) {
            Some((v, inserted)) if now.duration_since(*inserted) <= self.ttl => {
                // touch：把 key 移到 order 尾部（最近使用）。
                let mut order = self.order.lock().unwrap();
                if let Some(pos) = order.iter().position(|k| k == key) {
                    let k = order.remove(pos);
                    order.push(k);
                }
                Some(v.clone())
            }
            Some(_) => {
                // 过期：淘汰。
                map.remove(key);
                let mut order = self.order.lock().unwrap();
                order.retain(|k| k != key);
                None
            }
            None => None,
        }
    }

    pub fn put(&self, key: K, value: V) {
        let now = Instant::now();
        let mut map = self.map.lock().unwrap();
        let mut order = self.order.lock().unwrap();

        if map.contains_key(&key) {
            map.insert(key.clone(), (value, now));
            if let Some(pos) = order.iter().position(|k| *k == key) {
                let k = order.remove(pos);
                order.push(k);
            }
            return;
        }

        // 容量淘汰：移除最久未使用（order 头部）。
        if map.len() >= self.cap {
            if let Some(evict) = order.first().cloned() {
                order.remove(0);
                map.remove(&evict);
            }
        }
        map.insert(key.clone(), (value, now));
        order.push(key);
    }

    pub fn len(&self) -> usize {
        self.map.lock().unwrap().len()
    }

    pub fn clear(&self) {
        self.map.lock().unwrap().clear();
        self.order.lock().unwrap().clear();
    }
}

/// 生成缓存键：`tool:input` 的稳定哈希。
/// 仅用于缓存键（非安全用途），进程内一致性足够；碰撞概率对容量级缓存可忽略。
pub fn cache_key(parts: &[&str]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        p.hash(&mut h);
        h.write_u8(0xff);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_evicts_oldest_when_over_capacity() {
        let c = LruCache::new(2, Duration::from_secs(60));
        c.put("a", 1);
        c.put("b", 2);
        c.put("c", 3); // 超过容量，淘汰 a
        assert_eq!(c.get(&"a"), None);
        assert_eq!(c.get(&"b"), Some(2));
        assert_eq!(c.get(&"c"), Some(3));
    }

    #[test]
    fn ttl_expires_entries() {
        let c = LruCache::new(4, Duration::from_millis(10));
        c.put("k", "v");
        assert_eq!(c.get(&"k"), Some("v"));
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(c.get(&"k"), None);
    }

    #[test]
    fn touch_moves_to_most_recent() {
        let c = LruCache::new(2, Duration::from_secs(60));
        c.put("a", 1);
        c.put("b", 2);
        let _ = c.get(&"a"); // a 变最近
        c.put("c", 3); // 应淘汰 b
        assert_eq!(c.get(&"a"), Some(1));
        assert_eq!(c.get(&"b"), None);
        assert_eq!(c.get(&"c"), Some(3));
    }
}
