//! 可拓展层：工具注册、插件发现、技能固化（进化层）。
//!
//! - `Tool` trait + `ToolRegistry`：编译期/运行期均可注册能力。
//! - `tool!` 声明宏：一行把闭包注册成工具，零样板。
//! - `CommandTool` + `discover`：扫描 `plugins/*.json` 清单，把外部命令注册为工具，**无需重编译即可扩展**。
//! - `SkillBook`：把成功路径固化为技能（自进化），把失败沉淀为修正案例（自愈）。

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

/// 技能书（进化层）：成功路径固化 + 失败修正沉淀。
pub struct SkillBook {
    memory: DynMemory,
}

impl SkillBook {
    pub fn new(memory: DynMemory) -> Self {
        SkillBook { memory }
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
}

fn slug(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .take(40)
        .collect::<String>()
        .to_lowercase()
}

/// 注册内置工具（证明 `@tool`/宏机制可用）。
pub fn register_builtins(reg: &ToolRegistry) {
    reg.register(crate::tool!(echo, "回显输入，便于联调", |input: &Value| -> GanyuResult<Value> {
        Ok(input.clone())
    }));
    reg.register(crate::tool!(calc, "安全求值简单算术(+ - * / 与括号)", |input: &Value| -> GanyuResult<Value> {
        if !regex_fullmatch(r"[0-9+\-*/().\s]+", input.as_str()) {
            return Err(GanyuError::ToolFailed("calc".into(), "仅支持数字与 + - * / ( )".into()));
        }
        let r = eval_expr(input.as_str())?;
        Ok(Value(r.to_string()))
    }));
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

fn regex_fullmatch(pat: &str, s: &str) -> bool {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(pat).unwrap()).is_match(s)
}

/// 极简安全算术求值（仅 + - * / 与括号，f64）。无第三方依赖。
fn eval_expr(s: &str) -> GanyuResult<f64> {
    let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    let mut pos = 0usize;

    fn parse_expr(chars: &[char], pos: &mut usize) -> GanyuResult<f64> {
        let mut left = parse_term(chars, pos)?;
        while *pos < chars.len() {
            match chars[*pos] {
                '+' => {
                    *pos += 1;
                    left += parse_term(chars, pos)?;
                }
                '-' => {
                    *pos += 1;
                    left -= parse_term(chars, pos)?;
                }
                _ => break,
            }
        }
        Ok(left)
    }
    fn parse_term(chars: &[char], pos: &mut usize) -> GanyuResult<f64> {
        let mut left = parse_factor(chars, pos)?;
        while *pos < chars.len() && (chars[*pos] == '*' || chars[*pos] == '/') {
            let op = chars[*pos];
            *pos += 1;
            let right = parse_factor(chars, pos)?;
            if op == '*' {
                left *= right;
            } else {
                if right == 0.0 {
                    return Err(GanyuError::ToolFailed("calc".into(), "除零".into()));
                }
                left /= right;
            }
        }
        Ok(left)
    }
    fn parse_factor(chars: &[char], pos: &mut usize) -> GanyuResult<f64> {
        if *pos < chars.len() && chars[*pos] == '-' {
            *pos += 1;
            return Ok(-parse_factor(chars, pos)?);
        }
        if *pos < chars.len() && chars[*pos] == '(' {
            *pos += 1;
            let v = parse_expr(chars, pos)?;
            if *pos < chars.len() && chars[*pos] == ')' {
                *pos += 1;
            } else {
                return Err(GanyuError::ToolFailed("calc".into(), "括号不匹配".into()));
            }
            return Ok(v);
        }
        let start = *pos;
        while *pos < chars.len() && (chars[*pos].is_ascii_digit() || chars[*pos] == '.') {
            *pos += 1;
        }
        if start == *pos {
            return Err(GanyuError::ToolFailed("calc".into(), "无效数字".into()));
        }
        chars[start..*pos]
            .iter()
            .collect::<String>()
            .parse::<f64>()
            .map_err(|_| GanyuError::ToolFailed("calc".into(), "数字解析失败".into()))
    }

    let r = parse_expr(&chars, &mut pos)?;
    if pos != chars.len() {
        return Err(GanyuError::ToolFailed("calc".into(), "多余字符".into()));
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tool_registry_echo() {
        let reg = ToolRegistry::new();
        register_builtins(&reg);
        let out = reg.call("echo", &Value("hi".into())).await.unwrap();
        assert_eq!(out, Value("hi".into()));
    }

    #[tokio::test]
    async fn tool_calc_arithmetic() {
        let reg = ToolRegistry::new();
        register_builtins(&reg);
        let out = reg.call("calc", &Value("(1+2)*3".into())).await.unwrap();
        assert_eq!(out, Value("9".to_string()));
    }

    #[test]
    fn skill_slug_is_stable() {
        assert_eq!(slug("上月 华东 利润"), "上月华东利润");
    }
}
