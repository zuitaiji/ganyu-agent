//! 可拓展层：工具注册、插件发现、技能固化（进化层）。
//!
//! - `Tool` trait + `ToolRegistry`：编译期/运行期均可注册能力。
//! - `tool!` 声明宏：一行把闭包注册成工具，零样板。
//! - `CommandTool` + `discover`：扫描 `plugins/*.json` 清单，把外部命令注册为工具，**无需重编译即可扩展**。
//! - `SkillBook`：成功路径固化为案例（自进化）+ 失败沉淀为修正（自愈）+ 容纳可生长的特性技能。
//! - `builtins` / `skills`：具体的内置工具与内置技能实现。

pub mod builtins;
pub mod mcp;
pub mod nomifun_caps;
pub mod skills;

pub use skills::{Skill, SkillStep, SkillTool};

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::cache::{cache_key, LruCache};
use crate::core::memory::{DynMemory, MemoryHit};
use crate::error::{GanyuError, GanyuResult};
use crate::heal::with_retry_async;
use crate::observe::{AuditEvent, AuditLog};
use crate::value::Value;

pub type DynTool = Arc<dyn Tool + Send + Sync>;

/// 工具抽象：名字 + 描述 + 异步执行（输入/输出统一为 `Value`）。
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn invoke(&self, input: &Value) -> GanyuResult<Value>;

    /// 是否为「副作用工具」（写文件/执行命令/写记忆等）。
    /// 默认 `false`（只读）。`true` 的工具在 `ToolRegistry::call` 中**不被盲目重试**（M3），
    /// 避免重复写、重复发、重复执行等放大故障。
    fn side_effecting(&self) -> bool {
        false
    }

    /// 结构化工具描述（M6 原生函数调用）：供接入真实模型时生成 tool schema。
    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name(),
            "description": self.description(),
            "parameters": { "type": "object", "properties": { "input": { "type": "string" } } }
        })
    }
}

pub struct ToolRegistry {
    tools: Mutex<HashMap<String, DynTool>>,
    /// 只读工具结果缓存（LRU+TTL；默认 None=关，显式 `enable_tool_cache` 开启）。
    cache: Mutex<Option<LruCache<u64, Value>>>,
    /// 审计日志（默认 None=关）。
    audit: Mutex<Option<Arc<AuditLog>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        ToolRegistry {
            tools: Mutex::new(HashMap::new()),
            cache: Mutex::new(None),
            audit: Mutex::new(None),
        }
    }

    /// 开启只读工具结果缓存（缓存优化：calc/echo/file_read 等幂等结果复用）。
    /// 副作用工具（`side_effecting`）永不缓存。`ttl` 为 0 视为关闭。
    pub fn enable_tool_cache(&self, ttl: Duration) {
        if ttl > Duration::ZERO {
            *self.cache.lock().unwrap() = Some(LruCache::new(256, ttl));
        }
    }

    /// 挂接审计日志（可观测性/合规）。
    pub fn set_audit(&self, log: Arc<AuditLog>) {
        *self.audit.lock().unwrap() = Some(log);
    }

    fn audit_evt(&self, ev: AuditEvent) {
        if let Some(a) = self.audit.lock().unwrap().as_ref() {
            a.event(ev);
        }
    }

    pub fn register(&self, tool: DynTool) {
        self.tools.lock().unwrap().insert(tool.name().to_string(), tool);
    }

    /// 调用工具。自愈：失败自动重试（指数退避）。
    ///
    /// M3：副作用工具（`side_effecting()==true`）不做盲目重试——失败即失败，
    /// 防止重复写/重复发/重复执行放大故障。只读工具才享受重试自愈。
    ///
    /// 缓存：只读工具开启缓存时，同 `tool:input` 在 TTL 内直接命中；
    /// 副作用工具永不缓存，防止陈旧状态复现。
    pub async fn call(&self, name: &str, input: &Value) -> GanyuResult<Value> {
        let tool = self
            .tools
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| GanyuError::ToolNotFound(name.to_string()))?;

        // 缓存键仅对「只读且缓存已开启」的工具生成。
        let cache_on = self.cache.lock().unwrap().is_some();
        let key = if cache_on && !tool.side_effecting() {
            Some(cache_key(&[name, input.as_str()]))
        } else {
            None
        };
        if let Some(k) = &key {
            if let Some(hit) = self.cache.lock().unwrap().as_ref().unwrap().get(k) {
                self.audit_evt(AuditEvent::ToolCacheHit { tool: name });
                return Ok(hit);
            }
        }

        let start = Instant::now();
        let result = if tool.side_effecting() {
            tool.invoke(input)
                .await
                .map_err(|e| GanyuError::ToolFailed(name.to_string(), format!("{e:?}")))
        } else {
            with_retry_async(|| tool.invoke(input), 3, Duration::from_millis(50))
                .await
                .map_err(|e| GanyuError::ToolFailed(name.to_string(), format!("{e:?}")))
        };
        let ms = start.elapsed().as_millis() as u64;
        self.audit_evt(AuditEvent::ToolCall { tool: name, ok: result.is_ok(), ms });
        if let Err(GanyuError::Forbidden(reason)) = &result {
            self.audit_evt(AuditEvent::SecurityDenial {
                kind: "tool_forbidden",
                reason,
            });
        }

        if let (Ok(v), Some(k)) = (&result, &key) {
            self.cache.lock().unwrap().as_ref().unwrap().put(*k, v.clone());
        }
        result
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.lock().unwrap().keys().cloned().collect()
    }

    pub fn get_description(&self, name: &str) -> Option<String> {
        self.tools
            .lock()
            .unwrap()
            .get(name)
            .map(|t| t.description().to_string())
    }

    /// 插件发现：扫描 `dir` 下 `*.json` 清单，注册 `command` 类外部工具。
    ///
    /// C2 失败闭环：
    /// - 默认**不**扫描（需 `GANYU_ALLOW_PLUGINS=1` 显式开启）；
    /// - 每个清单项必须显式 `vetted: true`；
    /// - 命令的程序名必须在 `GANYU_PLUGIN_ALLOW` 允许清单内（缺省为空=全拒）；
    /// - 程序名不得含 shell 元字符/绝对路径/穿越（`is_safe_program`）。
    pub fn discover(&self, dir: &Path) -> GanyuResult<usize> {
        if std::env::var("GANYU_ALLOW_PLUGINS").as_deref() != Ok("1") {
            return Ok(0);
        }
        if !dir.exists() {
            return Ok(0);
        }
        let allow = plugin_allowlist();
        let mut count = 0;
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let manifest: serde_json::Value = serde_json::from_reader(std::fs::File::open(&path)?)?;
            if let Some(arr) = manifest.as_array() {
                for spec in arr {
                    let vetted = spec["vetted"].as_bool().unwrap_or(false);
                    if !vetted {
                        continue;
                    }
                    let name = spec["name"].as_str().unwrap_or("").to_string();
                    let command = spec["command"].as_str().unwrap_or("").to_string();
                    let desc = spec["description"].as_str().unwrap_or("").to_string();
                    if name.is_empty() || command.is_empty() {
                        continue;
                    }
                    let prog = command.split_whitespace().next().unwrap_or("").to_string();
                    // C2 失败闭环：白名单缺省空 = 全拒（空 allowlist 不得放行任何程序）。
                    if allow.is_empty() || !allow.contains(&prog) {
                        continue;
                    }
                    if !is_safe_program(&prog) {
                        continue;
                    }
                    self.register(Arc::new(CommandTool {
                        name,
                        command,
                        description: desc,
                    }));
                    count += 1;
                }
            }
        }
        Ok(count)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 外部命令插件：输入经 stdin 喂给子进程，stdout 作为 `Value` 返回。
pub struct CommandTool {
    name: String,
    command: String,
    description: String,
}

#[async_trait]
impl Tool for CommandTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn side_effecting(&self) -> bool {
        true
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;
        use std::process::Stdio;

        // 命令可带参数：按空白拆分为 program + args。
        let mut parts = self.command.split_whitespace();
        let prog = parts
            .next()
            .ok_or_else(|| GanyuError::Plugin("empty command".into()))?;
        let args: Vec<&str> = parts.collect();

        let mut child = Command::new(prog)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| GanyuError::Plugin(format!("spawn {}: {e}", self.command)))?;
        if let Some(mut stdin) = child.stdin.take() {
            let data = input.as_str().as_bytes().to_vec();
            tokio::spawn(async move {
                let _ = stdin.write_all(&data).await;
                let _ = stdin.flush().await;
            });
        }
        // 超时等待，防插件命令卡死挂线程（30s）。
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| GanyuError::Plugin(format!("{} 执行超时（30s 上限）", self.command)))?
        .map_err(|e| GanyuError::Plugin(e.to_string()))?;
        if !output.status.success() {
            return Err(GanyuError::Plugin(format!(
                "{} exit {}: {}",
                self.command,
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        // 输出截断（防结果膨胀，1MB 上限）
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        const MAX_OUT: usize = 1024 * 1024;
        let text = if s.chars().count() > MAX_OUT {
            format!("{}…[已截断：输出超过 1MB 上限]", s.chars().take(MAX_OUT).collect::<String>())
        } else {
            s
        };
        Ok(Value(text))
    }
}

/// 技能书（进化层）：成功案例固化 + 失败修正沉淀 + 可生长的特性技能。
pub struct SkillBook {
    memory: DynMemory,
    skills: Mutex<Vec<Skill>>,
}

impl SkillBook {
    pub fn new(memory: DynMemory) -> Self {
        SkillBook {
            memory,
            skills: Mutex::new(Vec::new()),
        }
    }

    /// 把一次成功（intent → action → result）写回 `agent/memory/cases`（自进化）。
    pub async fn capture(&self, intent: &str, action: &str, result: &Value) -> GanyuResult<()> {
        let uri = format!("viking://agent/memory/cases/{}", slug(intent));
        let payload = Value(
            serde_json::json!({
                "intent": intent,
                "action": action,
                "result": result.as_str(),
            })
            .to_string(),
        );
        self.memory.put(&uri, &payload).await
    }

    pub async fn lookup(&self, intent: &str) -> GanyuResult<Vec<MemoryHit>> {
        self.memory.search(intent, "viking://agent/memory/cases").await
    }

    /// 自愈：失败踪迹沉淀，供下次规避。
    pub async fn heal_from_failure(&self, intent: &str, error: &str) -> GanyuResult<()> {
        let uri = format!("viking://agent/memory/failures/{}", slug(intent));
        let payload = Value(
            serde_json::json!({ "intent": intent, "error": error }).to_string(),
        );
        self.memory.put(&uri, &payload).await
    }

    // ---- 特性技能（可生长） ----

    /// 注册一个内置/外部特性技能。
    pub fn register_skill(&self, skill: Skill) {
        self.skills.lock().unwrap().push(skill);
    }

    /// 取出某个技能的副本（供 `SkillTool` 执行）。
    pub fn get_skill(&self, name: &str) -> Option<Skill> {
        self.skills
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.name == name)
            .cloned()
    }

    pub fn skill_names(&self) -> Vec<String> {
        self.skills.lock().unwrap().iter().map(|s| s.name.clone()).collect()
    }

    pub fn skill_specs(&self) -> Vec<String> {
        self.skills
            .lock()
            .unwrap()
            .iter()
            .map(|s| format!("skill:{} - {}", s.name, s.description))
            .collect()
    }

    /// 关键字路由：把自然语言意图落到已注册技能（无命中返回 None）。
    pub fn match_intent(&self, query: &str) -> Option<String> {
        let q = query.to_lowercase();
        let rules: &[(&str, &str)] = &[
            ("总结", "summarize"),
            ("摘要", "summarize"),
            ("summarize", "summarize"),
            ("排查", "troubleshoot"),
            ("故障", "troubleshoot"),
            ("报错", "troubleshoot"),
            ("troubleshoot", "troubleshoot"),
            ("error", "troubleshoot"),
            ("知识库", "kb_query"),
            ("kb_query", "kb_query"),
            ("查一下", "kb_query"),
        ];
        for (kw, skill) in rules {
            if q.contains(kw) && self.get_skill(skill).is_some() {
                return Some(skill.to_string());
            }
        }
        // nomifun 内置 agent 能力全量路由：命中即派发到对应 skill。
        if let Some(skill) = crate::ext::nomifun_caps::match_nomifun_intent(&q) {
            if self.get_skill(&skill).is_some() {
                return Some(skill);
            }
        }
        None
    }
}

fn slug(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .take(40)
        .collect::<String>()
        .to_lowercase()
}

/// 解析 `GANYU_PLUGIN_ALLOW`（逗号分隔）为允许的程序名清单；缺省为空=全拒。
fn plugin_allowlist() -> Vec<String> {
    std::env::var("GANYU_PLUGIN_ALLOW")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// C2：程序名必须是「安全 token」——仅字母数字/点/下划线/相对分隔符，
/// 不得含 shell 元字符、不得为绝对路径、不得含 `..` 穿越。
fn is_safe_program(prog: &str) -> bool {
    if prog.is_empty() || prog.contains("..") {
        return false;
    }
    // F-08：禁止把 shell 解释器当插件命令——shell 可直接执行任意命令，违背白名单语义。
    let lower = prog.to_lowercase();
    if matches!(
        lower.as_str(),
        "sh" | "bash" | "cmd" | "powershell" | "pwsh" | "zsh" | "fish"
            | "sh.exe" | "bash.exe" | "cmd.exe" | "powershell.exe" | "pwsh.exe"
            | "zsh.exe" | "fish.exe"
    ) {
        return false;
    }
    if prog.starts_with('/') || prog.starts_with('\\') || prog.contains(':') {
        return false; // 拒绝绝对路径 / 盘符
    }
    prog.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '/' || c == '-')
}

/// 声明宏：把闭包注册为工具。`body` 类型为 `Fn(&Value) -> GanyuResult<Value>`。
#[macro_export]
macro_rules! tool {
    ($name:ident, $desc:expr, $body:expr) => {{
        #[allow(non_camel_case_types)]
        struct $name;
        #[async_trait::async_trait]
        impl $crate::ext::Tool for $name {
            fn name(&self) -> &str {
                stringify!($name)
            }
            fn description(&self) -> &str {
                $desc
            }
            async fn invoke(&self, input: &$crate::value::Value) -> $crate::error::GanyuResult<$crate::value::Value> {
                ($body)(input)
            }
        }
        std::sync::Arc::new($name) as std::sync::Arc<dyn $crate::ext::Tool + Send + Sync>
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn skill_slug_is_stable() {
        assert_eq!(slug("上月 华东 利润"), "上月华东利润");
    }

    #[test]
    fn skill_book_register_and_match() {
        let book = SkillBook::new(Arc::new(crate::core::memory::LocalMemory::new(
            ".ganyu_skillbook_test_mem.json",
        )));
        book.register_skill(Skill {
            name: "summarize".into(),
            description: "摘要".into(),
            steps: vec![],
        });
        assert_eq!(book.match_intent("帮我总结一下"), Some("summarize".to_string()));
        assert_eq!(book.match_intent("今天天气"), None);
        let _ = std::fs::remove_file(".ganyu_skillbook_test_mem.json");
    }

    // 改 env 的测试必须串行（全局 env + 并行竞态，历史 flaky 教训）。
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn plugin_fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ganyu_plug_{}_{}", name, std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("m.json"),
            r#"[{"name":"x","command":"echo hi","description":"t","vetted":true}]"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn plugins_default_deny_when_allowlist_empty() {
        // P2：GANYU_PLUGIN_ALLOW 缺省空 = 全拒（C2 fail-closed），不得因空白名单放行。
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("GANYU_ALLOW_PLUGINS", "1");
        std::env::remove_var("GANYU_PLUGIN_ALLOW");
        let dir = plugin_fixture("deny");
        let reg = ToolRegistry::new();
        let n = reg.discover(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(n, 0, "白名单为空时必须全拒（缺省安全）");
    }

    #[test]
    fn plugins_allow_when_prog_in_allowlist() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("GANYU_ALLOW_PLUGINS", "1");
        std::env::set_var("GANYU_PLUGIN_ALLOW", "echo");
        let dir = plugin_fixture("allow");
        let reg = ToolRegistry::new();
        let n = reg.discover(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(n, 1, "程序名在白名单内应注册");
    }

    #[test]
    fn plugins_deny_unvetted_or_unlisted() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("GANYU_ALLOW_PLUGINS", "1");
        std::env::set_var("GANYU_PLUGIN_ALLOW", "echo");
        let dir = std::env::temp_dir().join(format!("ganyu_plug_mix_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // 一条 vetted 但程序不在白名单（python），一条 vetted=false
        std::fs::write(
            dir.join("m.json"),
            r#"[{"name":"a","command":"python x.py","vetted":true},{"name":"b","command":"echo hi","vetted":false}]"#,
        )
        .unwrap();
        let reg = ToolRegistry::new();
        let n = reg.discover(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(n, 0, "未白名单程序与未 vetted 项均不得注册");
    }

    #[tokio::test]
    async fn readonly_tool_result_cached() {
        // tool! 宏的闭包处于 item 上下文，不能捕获环境变量 → 用显式 struct 实现。
        struct Counting {
            n: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl Tool for Counting {
            fn name(&self) -> &str { "counting" }
            fn description(&self) -> &str { "计数回显（只读，可缓存）" }
            async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
                self.n.fetch_add(1, Ordering::SeqCst);
                Ok(input.clone())
            }
        }
        let reg = ToolRegistry::new();
        reg.enable_tool_cache(Duration::from_secs(60));
        let calls = Arc::new(AtomicUsize::new(0));
        reg.register(Arc::new(Counting { n: calls.clone() }));
        assert_eq!(
            reg.call("counting", &Value("x".into())).await.unwrap(),
            Value("x".into())
        );
        assert_eq!(
            reg.call("counting", &Value("x".into())).await.unwrap(),
            Value("x".into())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "第二次应命中缓存");
    }

    #[tokio::test]
    async fn side_effecting_tool_never_cached() {
        // 副作用工具即使缓存开启也不得缓存（防陈旧状态复现）。
        struct Sink {
            n: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl Tool for Sink {
            fn name(&self) -> &str { "sink" }
            fn description(&self) -> &str { "副作用写工具（测试）" }
            fn side_effecting(&self) -> bool { true }
            async fn invoke(&self, _: &Value) -> GanyuResult<Value> {
                self.n.fetch_add(1, Ordering::SeqCst);
                Ok(Value("done".into()))
            }
        }
        let reg = ToolRegistry::new();
        reg.enable_tool_cache(Duration::from_secs(60));
        let n = Arc::new(AtomicUsize::new(0));
        reg.register(Arc::new(Sink { n: n.clone() }));
        let _ = reg.call("sink", &Value("a".into())).await.unwrap();
        let _ = reg.call("sink", &Value("a".into())).await.unwrap();
        assert_eq!(n.load(Ordering::SeqCst), 2, "副作用工具必须每次都真正执行");
    }
}
