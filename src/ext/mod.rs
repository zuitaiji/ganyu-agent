//! 可拓展层：工具注册、插件发现、技能固化（进化层）。
//!
//! - `Tool` trait + `ToolRegistry`：编译期/运行期均可注册能力。
//! - `tool!` 声明宏：一行把闭包注册成工具，零样板。
//! - `CommandTool` + `discover`：扫描 `plugins/*.json` 清单，把外部命令注册为工具，**无需重编译即可扩展**。
//! - `SkillBook`：成功路径固化为案例（自进化）+ 失败沉淀为修正（自愈）+ 容纳可生长的特性技能。
//! - `builtins` / `skills`：具体的内置工具与内置技能实现。

pub mod builtins;
pub mod skills;

pub use skills::{Skill, SkillStep, SkillTool};

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::core::memory::{DynMemory, MemoryHit};
use crate::error::{GanyuError, GanyuResult};
use crate::heal::with_retry_async;
use crate::value::Value;

pub type DynTool = Arc<dyn Tool + Send + Sync>;

/// 工具抽象：名字 + 描述 + 异步执行（输入/输出统一为 `Value`）。
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn invoke(&self, input: &Value) -> GanyuResult<Value>;
}

pub struct ToolRegistry {
    tools: Mutex<HashMap<String, DynTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        ToolRegistry {
            tools: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(&self, tool: DynTool) {
        self.tools.lock().unwrap().insert(tool.name().to_string(), tool);
    }

    /// 调用工具。自愈：失败自动重试（指数退避）。
    pub async fn call(&self, name: &str, input: &Value) -> GanyuResult<Value> {
        let tool = self
            .tools
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| GanyuError::ToolNotFound(name.to_string()))?;
        with_retry_async(|| tool.invoke(input), 3, std::time::Duration::from_millis(50))
            .await
            .map_err(|e| GanyuError::ToolFailed(name.to_string(), format!("{e:?}")))
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
    pub fn discover(&self, dir: &Path) -> GanyuResult<usize> {
        if !dir.exists() {
            return Ok(0);
        }
        let mut count = 0;
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let manifest: serde_json::Value = serde_json::from_reader(std::fs::File::open(&path)?)?;
            if let Some(arr) = manifest.as_array() {
                for spec in arr {
                    let name = spec["name"].as_str().unwrap_or("").to_string();
                    let command = spec["command"].as_str().unwrap_or("").to_string();
                    let desc = spec["description"].as_str().unwrap_or("").to_string();
                    if name.is_empty() || command.is_empty() {
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
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        use std::io::Write;
        use std::process::{Command, Stdio};

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
            stdin
                .write_all(input.as_str().as_bytes())
                .map_err(|e| GanyuError::Plugin(e.to_string()))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| GanyuError::Plugin(e.to_string()))?;
        if !out.status.success() {
            return Err(GanyuError::Plugin(format!(
                "{} exit {}: {}",
                self.command,
                out.status,
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
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
}
