//! 内置工具（可拓展层的具体能力）。
//!
//! 把规划里的「对话/执行面」落到可调用原子能力：回显、算术、文件读写列、本地执行、
//! 记忆存取与检索、联网抓取（network 特性）。统一经 `register_core_tools` 注册，
//! 全程离线可跑；`web_fetch` 仅在 `--features network` 下可用。

use async_trait::async_trait;
use std::sync::Arc;

use crate::core::memory::DynMemory;
use crate::error::{GanyuError, GanyuResult};
use crate::ext::{Tool, ToolRegistry};
use crate::security;
use crate::value::Value;

/// 注册全部内置工具。记忆类工具需要 `memory` 句柄。
pub fn register_core_tools(reg: &ToolRegistry, memory: DynMemory) {
    reg.register(crate::tool!(
        echo,
        "回显输入，便于联调与管道串联",
        |input: &Value| -> GanyuResult<Value> { Ok(input.clone()) }
    ));
    reg.register(crate::tool!(
        calc,
        "安全求值简单算术(+ - * / 与括号)",
        |input: &Value| -> GanyuResult<Value> {
            if !regex_fullmatch(r"[0-9+\-*/().\s]+", input.as_str()) {
                return Err(GanyuError::ToolFailed(
                    "calc".into(),
                    "仅支持数字与 + - * / ( )".into(),
                ));
            }
            let r = eval_expr(input.as_str())?;
            Ok(Value(r.to_string()))
        }
    ));

    reg.register(Arc::new(FileRead));
    reg.register(Arc::new(FileWrite));
    reg.register(Arc::new(FileList));
    // C1：exec 默认不编译进二进制（失败闭环）；需 `shell` 特性 + 运行时 `GANYU_ALLOW_SHELL=1`。
    #[cfg(feature = "shell")]
    reg.register(Arc::new(ExecTool));
    reg.register(Arc::new(RememberTool {
        memory: memory.clone(),
    }));
    reg.register(Arc::new(RecallTool {
        memory: memory.clone(),
    }));
    reg.register(Arc::new(RagTool { memory }));

    #[cfg(feature = "network")]
    register_network_tools(reg);
}

#[cfg(feature = "network")]
fn register_network_tools(reg: &ToolRegistry) {
    reg.register(Arc::new(WebFetch));
}

/// 读文件：输入为路径，输出为文件内容（统一字符串值）。
/// 容错：直接路径读取失败时，从输入中抽取路径类 token 再试（便于 NL 路由把整句当参数）。
pub struct FileRead;

#[async_trait]
impl Tool for FileRead {
    fn name(&self) -> &str {
        "file_read"
    }
    fn description(&self) -> &str {
        "读取沙箱内的文本文件（输入：相对路径；C3/C4 禁止穿越沙箱根）"
    }
    fn side_effecting(&self) -> bool {
        false
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        let raw = input.as_str().trim();
        // C3/C4：强制解析到沙箱根内，拒绝绝对路径/穿越/逃逸。
        let path = security::resolve_sandboxed(raw)?;
        if let Ok(text) = std::fs::read_to_string(&path) {
            return Ok(Value(text));
        }
        if let Some(p) = extract_path_token(raw) {
            let path2 = security::resolve_sandboxed(&p)?;
            if let Ok(text) = std::fs::read_to_string(&path2) {
                return Ok(Value(text));
            }
        }
        Err(GanyuError::ToolFailed(
            "file_read".into(),
            format!("{raw}: 文件未找到（沙箱内）"),
        ))
    }
}

/// 从一段文本中抽取首个「像路径」的 token（含 `.`/`/`/`\` 或常见扩展名）。
fn extract_path_token(s: &str) -> Option<String> {
    for tok in s.split_whitespace() {
        let t = tok.trim_matches(|c| "\"'`()[]{}".contains(c));
        if t.contains('.') || t.contains('/') || t.contains('\\') {
            return Some(t.to_string());
        }
    }
    None
}

/// 写文件：输入首行为路径，其余为内容。
pub struct FileWrite;

#[async_trait]
impl Tool for FileWrite {
    fn name(&self) -> &str {
        "file_write"
    }
    fn description(&self) -> &str {
        "写入沙箱内的文本文件（输入：首行相对路径，空一行，余下为内容）"
    }
    fn side_effecting(&self) -> bool {
        true
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        let s = input.as_str();
        let mut it = s.splitn(2, '\n');
        let path = it.next().unwrap_or("").trim().to_string();
        let content = it.next().unwrap_or("").to_string();
        if path.is_empty() {
            return Err(GanyuError::ToolFailed(
                "file_write".into(),
                "首行必须提供路径".into(),
            ));
        }
        // C3/C4：解析到沙箱根内，拒绝逃逸。
        let resolved = security::resolve_sandboxed(&path)?;
        let n = content.len();
        std::fs::write(&resolved, content)
            .map_err(|e| GanyuError::ToolFailed("file_write".into(), format!("{path}: {e}")))?;
        Ok(Value(format!("已写入 {path}（{n} 字节，沙箱内）")))
    }
}

/// 列目录：输入为目录路径。
pub struct FileList;

#[async_trait]
impl Tool for FileList {
    fn name(&self) -> &str {
        "file_list"
    }
    fn description(&self) -> &str {
        "列出沙箱内的目录条目（输入：相对目录路径）"
    }
    fn side_effecting(&self) -> bool {
        false
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        let dir = input.as_str().trim();
        // C3/C4：沙箱内目录列举，拒绝逃逸。
        let resolved = security::resolve_sandboxed(dir)?;
        let mut entries = Vec::new();
        for e in std::fs::read_dir(&resolved)
            .map_err(|e| GanyuError::ToolFailed("file_list".into(), format!("{dir}: {e}")))?
        {
            let e = e.map_err(|e| GanyuError::ToolFailed("file_list".into(), e.to_string()))?;
            let kind = if e.path().is_dir() { "d" } else { "f" };
            let name = e.file_name().to_string_lossy().to_string();
            entries.push(format!("{kind} {name}"));
        }
        entries.sort();
        Ok(Value(entries.join("\n")))
    }
}

/// 本地执行：输入为命令，经 `sh -c`（Unix）/`cmd /c`（Windows）运行，返回 stdout。
pub struct ExecTool;

#[async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }
    fn description(&self) -> &str {
        "在本机执行 shell 命令（默认关闭；需 shell 特性 + GANYU_ALLOW_SHELL=1）"
    }
    fn side_effecting(&self) -> bool {
        true
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        // C1 失败闭环：即使 `shell` 特性已编译，运行时仍需显式开启才放行。
        if !security::shell_allowed() {
            return Err(GanyuError::Forbidden(
                "exec 已禁用（默认关闭；需 shell 特性编译且设置 GANYU_ALLOW_SHELL=1 才放行）".into(),
            ));
        }

        let cmd_str = input.as_str().trim();
        let (prog, flag) = if cfg!(windows) {
            ("cmd", "/c")
        } else {
            ("sh", "-c")
        };
        let mut cmd = Command::new(prog);
        cmd.arg(flag).arg(cmd_str)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // H3：仅在子进程内套 Landlock FS 沙箱（Unix + sandbox 特性），约束派生进程，
        // 不波及 agent 主进程。非 Linux/未开启则为无操作。
        #[cfg(all(unix, feature = "sandbox"))]
        {
            let root = security::sandbox_root();
            unsafe {
                cmd.pre_exec(move || crate::sandbox::apply_fs_sandbox(&root));
            }
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| GanyuError::ToolFailed("exec".into(), e.to_string()))?;
        let _ = child.stdin.take().map(|mut s| s.write_all(b""));
        let out = child
            .wait_with_output()
            .map_err(|e| GanyuError::ToolFailed("exec".into(), e.to_string()))?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if !out.status.success() {
            return Err(GanyuError::ToolFailed(
                "exec".into(),
                format!("exit {}: {}", out.status, stderr),
            ));
        }
        Ok(Value(if stderr.is_empty() {
            stdout
        } else {
            format!("{stdout}\n[stderr] {stderr}")
        }))
    }
}

/// 记忆写入：输入首行为 key，余下为 value，存到 `viking://user/memory/<key>`。
pub struct RememberTool {
    memory: DynMemory,
}

#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        "remember"
    }
    fn description(&self) -> &str {
        "记住一条事实（输入：首行 key，空一行，余下为 value）"
    }
    fn side_effecting(&self) -> bool {
        true
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        let s = input.as_str();
        let mut it = s.splitn(2, '\n');
        let key = it.next().unwrap_or("").trim().to_string();
        let val = it.next().unwrap_or("").to_string();
        if key.is_empty() {
            return Err(GanyuError::ToolFailed(
                "remember".into(),
                "首行必须提供 key".into(),
            ));
        }
        let uri = format!("viking://user/memory/{key}");
        self.memory.put(&uri, &Value(val)).await?;
        Ok(Value(format!("已记住 {key}")))
    }
}

/// 记忆读取：输入为 key。
pub struct RecallTool {
    memory: DynMemory,
}

#[async_trait]
impl Tool for RecallTool {
    fn name(&self) -> &str {
        "recall"
    }
    fn description(&self) -> &str {
        "回忆一条事实（输入：key）"
    }
    fn side_effecting(&self) -> bool {
        false
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        let key = input.as_str().trim();
        let uri = format!("viking://user/memory/{key}");
        match self.memory.get(&uri).await? {
            Some(v) => Ok(v),
            None => Ok(Value(format!("未找到 {key}"))),
        }
    }
}

/// 记忆检索（RAG 雏形）：输入为查询，返回命中片段。
pub struct RagTool {
    memory: DynMemory,
}

#[async_trait]
impl Tool for RagTool {
    fn name(&self) -> &str {
        "rag_search"
    }
    fn description(&self) -> &str {
        "在记忆知识库中检索（输入：查询）"
    }
    fn side_effecting(&self) -> bool {
        false
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        let hits = self.memory.search(input.as_str(), "viking://").await?;
        if hits.is_empty() {
            return Ok(Value("无命中".into()));
        }
        let lines: Vec<String> = hits
            .iter()
            .map(|h| format!("- [{}] {}: {}", h.score, h.uri, h.l0))
            .collect();
        Ok(Value(lines.join("\n")))
    }
}

/// 联网抓取（仅 network 特性）：GET 一个 URL 返回文本。
#[cfg(feature = "network")]
pub struct WebFetch;

#[cfg(feature = "network")]
#[async_trait]
impl Tool for WebFetch {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "抓取网页/接口文本（输入：URL）"
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        let url = input.as_str().trim();
        // C5：出站前做 SSRF 防护（拒绝内网/环回/链路本地/云元数据等）。
        security::ssrf_guard(url)?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            // 关闭自动重定向：避免 30x 跳转到内网绕过入口校验。
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| GanyuError::BackendUnavailable(format!("web_fetch: {e}")))?;
        // 若服务端返回重定向，二次校验目标（防止重定向逃逸）。
        if resp.status().is_redirection() {
            if let Some(loc) = resp.headers().get(reqwest::header::LOCATION) {
                let loc = loc.to_str().unwrap_or("").to_string();
                security::ssrf_guard(&loc)?;
            } else {
                return Err(GanyuError::Ssrf("非法重定向（缺 Location）".into()));
            }
        }
        let text = resp
            .text()
            .await
            .map_err(|e| GanyuError::Http(e.to_string()))?;
        Ok(Value(text))
    }
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
    use crate::core::memory::LocalMemory;

    fn reg_with_mem() -> (ToolRegistry, DynMemory) {
        let mem: DynMemory = Arc::new(LocalMemory::new(".ganyu_builtin_test_mem.json"));
        let reg = ToolRegistry::new();
        register_core_tools(&reg, mem.clone());
        (reg, mem)
    }

    #[tokio::test]
    async fn echo_and_calc() {
        let (reg, _m) = reg_with_mem();
        assert_eq!(reg.call("echo", &Value("hi".into())).await.unwrap(), Value("hi".into()));
        assert_eq!(
            reg.call("calc", &Value("(1+2)*3".into())).await.unwrap(),
            Value("9".to_string())
        );
    }

    #[tokio::test]
    async fn file_roundtrip() {
        let (reg, _m) = reg_with_mem();
        let p = ".ganyu_builtin_test.txt";
        let _ = std::fs::remove_file(p);
        reg.call("file_write", &Value(format!("{p}\nhello world")))
            .await
            .unwrap();
        let got = reg.call("file_read", &Value(p.into())).await.unwrap();
        assert_eq!(got, Value("hello world".into()));
        let _ = std::fs::remove_file(p);
    }

    #[tokio::test]
    async fn remember_recall_rag() {
        let (reg, _m) = reg_with_mem();
        reg.call("remember", &Value("city\n杭州".into())).await.unwrap();
        let got = reg.call("recall", &Value("city".into())).await.unwrap();
        assert_eq!(got, Value("杭州".into()));
        let hits = reg.call("rag_search", &Value("杭州".into())).await.unwrap();
        assert!(hits.as_str().contains("杭州"));
        let _ = std::fs::remove_file(".ganyu_builtin_test_mem.json");
    }
}
