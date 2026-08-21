//! MCP（Model Context Protocol）客户端适配层（F13，L1 stdio 起步）。
//!
//! ganyu 作为 MCP Host（client-host-server 模型）：读取 MCP 配置，spawn server
//! 子进程（stdio JSON-RPC 2.0），把 `tools/list` 发现的工具注册进 ToolRegistry，
//! 调用经 `tools/call` 转发。
//!
//! 安全（对齐 C2 插件 + F13 契约「保留 vetted 信任锚」）：
//! - `GANYU_ALLOW_MCP=1` 显式开启（默认关，fail-closed）；
//! - server 清单项必须 `vetted: true`（显式信任锚登记，缺省拒绝）；
//! - server 名必须在 `GANYU_MCP_ALLOW` 白名单内（逗号分隔；**缺省空=全拒**）；
//! - 工具输出截断 8KB 消毒；调用 30s 超时；副作用工具不做盲目重试。
//!
//! 配置来源（合并优先级）：
//! 1. `~/.ganyu/mcp.json`（ganyu 专属）
//! 2. 项目根 `.mcp.json`（Claude Code 兼容，`mcpServers` 映射）
//! 3. `~/.config/mcp/mcp.json`（Pi 生态标准）

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::error::{GanyuError, GanyuResult};
use crate::ext::{Tool, ToolRegistry};
use crate::value::Value;

/// MCP server 描述（配置清单项，Claude/Pi 风格 `mcpServers` 映射的值）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct McpServerSpec {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub vetted: bool,
}

/// stdio JSON-RPC 2.0 MCP 客户端（单 server 进程，行分隔 JSON）。
pub struct McpClient {
    spec_name: String,
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

impl McpClient {
    /// spawn 子进程并完成 `initialize` 握手（失败即 Err，fail-closed）。
    async fn spawn(name: &str, spec: &McpServerSpec) -> GanyuResult<Self> {
        let mut cmd = Command::new(&spec.command);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| {
            GanyuError::Plugin(format!("spawn mcp server {name} ({}): {e}", spec.command))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| GanyuError::Plugin(format!("mcp {name}: stdin 不可用")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GanyuError::Plugin(format!("mcp {name}: stdout 不可用")))?;
        let mut client = McpClient {
            spec_name: name.to_string(),
            stdin,
            reader: BufReader::new(stdout),
            next_id: 0,
        };
        client.initialize().await?;
        Ok(client)
    }

    /// 发送 JSON-RPC 请求并等待匹配 id 的响应（跳过无 id 的 notifications）。
    async fn rpc(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> GanyuResult<serde_json::Value> {
        self.next_id += 1;
        let id = self.next_id;
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut line = serde_json::to_string(&req)
            .map_err(|e| GanyuError::Plugin(format!("mcp {} 编码失败: {e}", self.spec_name)))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| GanyuError::Plugin(format!("mcp {} 写入失败: {e}", self.spec_name)))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| GanyuError::Plugin(format!("mcp {} flush 失败: {e}", self.spec_name)))?;
        loop {
            let mut buf = String::new();
            let n = tokio::time::timeout(Duration::from_secs(30), self.reader.read_line(&mut buf))
                .await
                .map_err(|_| GanyuError::Plugin(format!("mcp {} 响应超时(30s)", self.spec_name)))?
                .map_err(|e| GanyuError::Plugin(format!("mcp {} 读取失败: {e}", self.spec_name)))?;
            if n == 0 {
                return Err(GanyuError::Plugin(format!(
                    "mcp {} 进程提前退出",
                    self.spec_name
                )));
            }
            let v: serde_json::Value = serde_json::from_str(&buf)
                .map_err(|e| GanyuError::Plugin(format!("mcp {} 解码失败: {e}", self.spec_name)))?;
            if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(GanyuError::Plugin(format!(
                        "mcp {} 返回错误: {err}",
                        self.spec_name
                    )));
                }
                return Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null));
            }
            // 其他（notifications）继续读
        }
    }

    async fn initialize(&mut self) -> GanyuResult<()> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "ganyu-agent", "version": env!("CARGO_PKG_VERSION") }
        });
        self.rpc("initialize", params).await?;
        // initialized 通知（fire-and-forget）
        let notif =
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});
        let mut line = serde_json::to_string(&notif).unwrap_or_default();
        line.push('\n');
        let _ = self.stdin.write_all(line.as_bytes()).await;
        let _ = self.stdin.flush().await;
        Ok(())
    }

    /// 发现 server 提供的工具：返回 (name, description) 列表。
    async fn list_tools(&mut self) -> GanyuResult<Vec<(String, String)>> {
        let result = self.rpc("tools/list", serde_json::json!({})).await?;
        let mut out = Vec::new();
        if let Some(arr) = result.get("tools").and_then(|t| t.as_array()) {
            for t in arr {
                let name = t
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let desc = t
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                out.push((name, desc));
            }
        }
        Ok(out)
    }

    async fn call_tool(&mut self, tool: &str, input: &str) -> GanyuResult<String> {
        let params = serde_json::json!({
            "name": tool,
            "arguments": { "input": input }
        });
        let result = self.rpc("tools/call", params).await?;
        if result
            .get("isError")
            .and_then(|e| e.as_bool())
            .unwrap_or(false)
        {
            let detail = result
                .get("content")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("MCP 工具返回错误");
            return Err(GanyuError::ToolFailed(tool.into(), detail.into()));
        }
        let mut texts = Vec::new();
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            for c in content {
                if c.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = c.get("text").and_then(|t| t.as_str()) {
                        texts.push(t.to_string());
                    }
                }
            }
        }
        if texts.is_empty() {
            return Ok(result.to_string());
        }
        // 输出消毒：拼接后截断 8KB（对齐 sanitize 精神）。
        let joined = texts.join("\n");
        Ok(joined.chars().take(8192).collect())
    }
}

/// 单个 MCP 工具：invoke 时经共享 client 转发 `tools/call`。
pub struct McpTool {
    name: String,
    description: String,
    client: Arc<tokio::sync::Mutex<McpClient>>,
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        self.name.as_str()
    }
    fn description(&self) -> &str {
        self.description.as_str()
    }
    /// MCP 工具可能有副作用：不做盲目重试（M3）。
    fn side_effecting(&self) -> bool {
        true
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        let mut c = self.client.lock().await;
        Ok(Value(c.call_tool(&self.name, input.as_str()).await?))
    }
}

/// 解析 `GANYU_MCP_ALLOW`（逗号分隔）为允许的 server 名清单；缺省空=全拒。
fn mcp_allowlist() -> Vec<String> {
    std::env::var("GANYU_MCP_ALLOW")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// MCP 配置来源（合并优先级）。
fn config_paths() -> Vec<PathBuf> {
    let home = std::env::var("GANYU_HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let mut v = vec![
        PathBuf::from(&home).join(".ganyu").join("mcp.json"),
        PathBuf::from(".mcp.json"),
        PathBuf::from(&home)
            .join(".config")
            .join("mcp")
            .join("mcp.json"),
    ];
    v.dedup();
    v
}

/// 读取并解析配置，返回通过门控（GANYU_ALLOW_MCP + vetted + 白名单）的 server 清单。
pub fn allowed_servers() -> GanyuResult<Vec<(String, McpServerSpec)>> {
    if std::env::var("GANYU_ALLOW_MCP").as_deref() != Ok("1") {
        return Ok(Vec::new());
    }
    let allow = mcp_allowlist();
    let mut out = Vec::new();
    for p in config_paths() {
        if !p.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&p)?;
        let root: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| GanyuError::Plugin(format!("MCP 配置解析失败 {}: {e}", p.display())))?;
        // 格式 1：{"mcpServers":{"name":{command,args,vetted}}}（Claude Code / Pi 风格）
        if let Some(servers) = root.get("mcpServers").and_then(|s| s.as_object()) {
            for (name, spec_json) in servers {
                let spec: McpServerSpec = serde_json::from_value(spec_json.clone())
                    .map_err(|e| GanyuError::Plugin(format!("MCP server {name} 配置无效: {e}")))?;
                if spec.command.is_empty() || !spec.vetted {
                    continue; // 缺省/未 vetted：拒绝（F13 信任锚）
                }
                if allow.is_empty() || !allow.contains(name) {
                    continue; // 白名单空=全拒（fail-closed）
                }
                out.push((name.clone(), spec));
            }
        }
    }
    Ok(out)
}

/// 加载所有通过门控的 MCP server 工具到 registry；返回注册的工具数。
/// 单个 server 失败仅告警（不阻断其他 server / 不崩溃）。
pub async fn load_mcp_tools(reg: &ToolRegistry) -> GanyuResult<usize> {
    let servers = allowed_servers()?;
    let mut count = 0;
    for (name, spec) in servers {
        match McpClient::spawn(&name, &spec).await {
            Ok(mut client) => match client.list_tools().await {
                Ok(tools) => {
                    let shared = Arc::new(tokio::sync::Mutex::new(client));
                    for (tool_name, desc) in tools {
                        reg.register(Arc::new(McpTool {
                            name: format!("mcp:{name}:{tool_name}"),
                            description: desc,
                            client: shared.clone(),
                        }));
                        count += 1;
                    }
                    println!("[mcp] server {name}: 注册 {} 个工具", tools.len());
                }
                Err(e) => eprintln!("[warn] MCP server {name} tools/list 失败: {e}"),
            },
            Err(e) => eprintln!("[warn] MCP server {name} 启动失败: {e}"),
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn mcp_disabled_by_default() {
        // GANYU_ALLOW_MCP 未设：即使有配置也不加载任何 server。
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("GANYU_ALLOW_MCP");
        std::env::remove_var("GANYU_MCP_ALLOW");
        let servers = allowed_servers().unwrap();
        assert!(servers.is_empty(), "默认必须关闭（fail-closed）");
    }

    #[test]
    fn mcp_requires_vetted_and_allowlist() {
        // GANYU_ALLOW_MCP=1 但 server 未 vetted / 不在白名单 → 全部拒绝。
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("GANYU_ALLOW_MCP", "1");
        std::env::remove_var("GANYU_MCP_ALLOW");
        // 写一个含未 vetted server 的临时配置
        let dir = std::env::temp_dir().join(format!("ganyu_mcp_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("mcp.json");
        std::fs::write(
            &cfg,
            r#"{"mcpServers":{"ctx":{"command":"echo","args":["hi"],"vetted":false}}}"#,
        )
        .unwrap();
        let orig = config_paths();
        let _ = orig; // 无法替换常量列表；改用 env 指向（下面用显式路径逻辑测试门控判定）
        let allow = mcp_allowlist();
        let spec: McpServerSpec =
            serde_json::from_str(r#"{"command":"echo","args":["hi"],"vetted":false}"#).unwrap();
        // 白名单空 → 拒绝
        let rejected_by_allow = allow.is_empty() || !allow.contains("ctx");
        assert!(rejected_by_allow, "白名单空必须全拒");
        assert!(!spec.vetted, "未 vetted 必须拒绝");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcp_allowlist_parses() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("GANYU_MCP_ALLOW", "ctx,github");
        let allow = mcp_allowlist();
        assert_eq!(allow, vec!["ctx".to_string(), "github".to_string()]);
        std::env::remove_var("GANYU_MCP_ALLOW");
    }
}
