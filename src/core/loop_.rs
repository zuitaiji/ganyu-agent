//! 推理循环（ReAct）：把对话面升级为「感知—推理—行动—观察」的多步 agent 工作流。
//!
//! 对齐开源 agent 的核心循环：Plan / Act / Observe。由可插拔 `Reasoner` 驱动每一步决策：
//! - `LocalReasoner`：离线确定性决策（`@tool arg` 脚本语法 + 关键字路由到技能），保证零网络可跑。
//! - `LlmReasoner`：接真实模型（经网关）做单步决策——输出 `@tool arg` 则行动，否则直接作答。
//!
//! 这是「全量流程」的主心骨：单条消息可被拆成多步工具调用，每步产出 Observation 再反思，
//! 直到 `Final`。失败的工具调用会作为 Observation 回流（自愈：agent 据此调整而非崩溃）。

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::llm::Message;
use crate::error::GanyuResult;
use crate::routing::Gateway;

/// 一次推理轨迹的原子步骤（可序列化为 JSON 落到会话记忆，供可观测与续接）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Step {
    /// 推理/思考（离线时记录人格与意图路由）。
    Thought(String),
    /// 调用某个工具（普通工具或 `skill:` 技能）。
    Action { tool: String, args: String },
    /// 工具返回（或失败文本，自愈回流）。
    Observation(String),
    /// 最终作答。
    Final(String),
}

/// 推理器的单步决策。
#[derive(Debug)]
pub enum Decision {
    Final(String),
    Act {
        tool: String,
        args: String,
        /// 同一脚本中本行之后的剩余内容；为空表示这是最后一步。
        remaining: String,
    },
}

/// 推理器抽象：把当前用户消息映射为下一步决策。可插拔，便于离线/联网两套实现。
#[async_trait]
pub trait Reasoner: Send + Sync {
    async fn decide(&self, user_msg: &str, known: &HashSet<String>) -> GanyuResult<Decision>;
}

/// 离线推理器：解析 `@tool arg` 脚本；多行脚本逐行执行，最后一行若无 `@` 则收尾。
///
/// 离线即可演示完整的多步循环；接真模型时由 `LlmReasoner` 取代，能力自动升级。
pub struct LocalReasoner;

#[async_trait]
impl Reasoner for LocalReasoner {
    async fn decide(&self, user_msg: &str, known: &HashSet<String>) -> GanyuResult<Decision> {
        if let Some((tool, args, remaining)) = parse_known_tool(user_msg, known) {
            return Ok(Decision::Act { tool, args, remaining });
        }
        Ok(Decision::Final(default_fallback(user_msg)))
    }
}

/// 联网推理器：把决策交给真实模型（经网关），模型输出 `@tool arg` 则行动，否则直接作答。
///
/// 指令注入已知工具清单；单步决策——模型决定是否调用工具或直接回答
/// （对齐 OpenClaw/Hermes 的 tool-calling 循环；ganyu 保持极简协议，避免依赖厂商 schema）。
pub struct LlmReasoner {
    gateway: Arc<Gateway>,
    /// 稳定 system 前缀缓存：(工具清单签名, system 消息)。
    /// 工具集不变时复用同一条 system，保证模型侧前缀缓存（DeepSeek/Anthropic 自动
    /// context caching）持续命中——对标 jcode 的「仅追加式上下文 + 固定系统提示」。
    sys_cache: std::sync::Mutex<Option<(String, String)>>,
}

impl LlmReasoner {
    pub fn new(gateway: Arc<Gateway>) -> Self {
        LlmReasoner {
            gateway,
            sys_cache: std::sync::Mutex::new(None),
        }
    }

    /// 构建（并缓存）稳定 system 提示：
    /// - 工具清单**排序**后固定拼接（HashSet 迭代顺序不稳定，必须排序保证前缀字节级稳定）；
    /// - 模板固定，动态部分一律留在 user 消息（不污染缓存前缀）。
    fn system_prompt(&self, known: &HashSet<String>) -> String {
        let mut tools: Vec<String> = known.iter().cloned().collect();
        tools.sort();
        let sig = tools.join(",");
        if let Some((cached_sig, cached_sys)) = self.sys_cache.lock().unwrap().as_ref() {
            if *cached_sig == sig {
                return cached_sys.clone();
            }
        }
        let sys = format!(
            "你是 ganyu 智能体的决策器。可用工具：{}。\
             若需要调用工具，请严格以 '@工具名 参数' 开头输出（例如 @calc 1+1）；\
             否则直接输出最终回答（不要输出 @ 开头的伪工具调用）。",
            sig
        );
        *self.sys_cache.lock().unwrap() = Some((sig, sys.clone()));
        sys
    }
}

#[async_trait]
impl Reasoner for LlmReasoner {
    async fn decide(&self, user_msg: &str, known: &HashSet<String>) -> GanyuResult<Decision> {
        // 强制工具指令：@tool / JSON 调用不经过模型，确定性执行。
        // 避免模型"假装"执行（如直接回复"已记住"而未真调 remember），
        // 与离线 LocalReasoner 行为对齐；模型只负责自由对话。
        if let Some((tool, args, remaining)) = parse_known_tool(user_msg, known) {
            return Ok(Decision::Act { tool, args, remaining });
        }
        // 稳定 system 前缀（排序工具清单 + 固定模板）→ 长会话模型侧前缀缓存命中。
        // 消息序：[system(稳定前缀), user(增量)] —— 动态内容只在 user 段追加。
        let sys = self.system_prompt(known);
        let messages = [
            Message::system(sys),
            Message::user(user_msg.to_string()),
        ];
        match self.gateway.complete(&messages).await {
            Ok(out) => {
                let text = out.as_str();
                if let Some((tool, args)) = parse_tool_call(text) {
                    if known.contains(&tool) {
                        return Ok(Decision::Act {
                            tool,
                            args,
                            remaining: String::new(),
                        });
                    }
                }
                Ok(Decision::Final(text.to_string()))
            }
            Err(e) => Ok(Decision::Final(format!("[模型暂不可用] {e}"))),
        }
    }
}

/// 解析单条工具调用（M6 原生函数调用 + 向后兼容 `@tool arg`）。
///
/// 支持两种形式：
/// - JSON：`{"tool":"x","args":"y"}` 或 OpenAI `function_call` 风格（`name`/`arguments`）；
/// - 脚本：`@tool arg`。
/// 返回 `(工具名, 参数)`；无法解析返回 `None`。
pub fn parse_tool_call(text: &str) -> Option<(String, String)> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    // JSON 原生函数调用
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
        if v.is_object() {
            let name = v
                .get("tool")
                .and_then(|x| x.as_str())
                .or_else(|| v.get("name").and_then(|x| x.as_str()))
                .map(|s| s.to_string());
            if let Some(name) = name {
                let args = v
                    .get("args")
                    .and_then(|x| x.as_str())
                    .or_else(|| v.get("arguments").and_then(|x| x.as_str()))
                    .unwrap_or("")
                    .to_string();
                return Some((name, args));
            }
        }
    }
    // `@tool arg` 脚本形式
    if let Some(rest) = t.strip_prefix('@') {
        let mut it = rest.splitn(2, char::is_whitespace);
        let tool = it.next().unwrap_or("").to_string();
        let args = it.next().unwrap_or("").trim().to_string();
        if !tool.is_empty() {
            return Some((tool, args));
        }
    }
    None
}

/// 去掉脚本中首个被命中的 `@tool` 行，返回剩余内容（供循环下一步）。
fn strip_first_tool_line(msg: &str, line: &str) -> String {
    let mut out = String::new();
    let mut skipped = false;
    for l in msg.lines() {
        if !skipped && l.trim() == line {
            skipped = true;
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    out.trim().to_string()
}

/// 解析 user_msg 中的工具指令（`@tool arg` 或 JSON），命中已知工具则返回
/// `(tool, args, remaining)`：
/// - **多行参数**：`@tool` 行之后、下一个 `@` 行之前的非 `@` 行并入 args
///   （file_write/remember 等"首行路径/键 + 内容"工具可直接用）；
/// - 下一个 `@` 行起（新的工具调用）及之后保留为 remaining，供循环继续执行。
/// 供 LocalReasoner（离线确定性）与 LlmReasoner（联网强制解析，杜绝模型"假装"执行）共用。
fn parse_known_tool(user_msg: &str, known: &HashSet<String>) -> Option<(String, String, String)> {
    for line in user_msg.lines() {
        let line = line.trim();
        if let Some((tool, args)) = parse_tool_call(line) {
            if known.contains(&tool) {
                let rest = strip_first_tool_line(user_msg, line);
                if rest.trim().is_empty() {
                    return Some((tool, args, String::new()));
                }
                let mut merged = args;
                let mut kept: Vec<&str> = Vec::new();
                let mut collecting = false;
                for l in rest.lines() {
                    if collecting {
                        kept.push(l);
                    } else if l.trim().starts_with('@') {
                        collecting = true;
                        kept.push(l);
                    } else {
                        if merged.is_empty() {
                            merged = l.trim_end().to_string();
                        } else {
                            merged = format!("{merged}\n{}", l.trim_end());
                        }
                    }
                }
                return Some((tool, merged, kept.join("\n")));
            }
        }
    }
    None
}

fn default_fallback(msg: &str) -> String {
    let preview: String = msg.chars().take(60).collect();
    format!(
        "[本地兜底] 收到：{preview}（未配置联网模型；用 @tool 驱动内置能力，或 --features network 接真模型进入多步深思）"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::Gateway;
    use std::sync::Arc;

    #[test]
    fn system_prompt_stable_across_order_and_calls() {
        let r = LlmReasoner::new(Arc::new(Gateway::new()));
        let mut a = HashSet::new();
        a.insert("calc".to_string());
        a.insert("echo".to_string());
        a.insert("web_fetch".to_string());
        let mut b = HashSet::new();
        b.insert("web_fetch".to_string());
        b.insert("echo".to_string());
        b.insert("calc".to_string());
        // 不同插入顺序 → 排序后相同前缀（缓存友好）
        assert_eq!(r.system_prompt(&a), r.system_prompt(&b));
        // 工具集变化 → 签名变化 → system 更新
        let mut c = a.clone();
        c.insert("exec".to_string());
        assert_ne!(r.system_prompt(&a), r.system_prompt(&c));
    }

    #[tokio::test]
    async fn local_reasoner_routes_tool() {
        let mut known = HashSet::new();
        known.insert("calc".to_string());
        let d = LocalReasoner
            .decide("@calc 1+1", &known)
            .await
            .unwrap();
        match d {
            Decision::Act { tool, args, remaining } => {
                assert_eq!(tool, "calc");
                assert_eq!(args, "1+1");
                assert!(remaining.is_empty());
            }
            _ => panic!("expected Act"),
        }
    }

    #[tokio::test]
    async fn local_reasoner_multistep_keeps_remaining() {
        let mut known = HashSet::new();
        known.insert("echo".to_string());
        known.insert("calc".to_string());
        let script = "@echo step1\n@calc 2+2\n收尾";
        let d = LocalReasoner.decide(script, &known).await.unwrap();
        match d {
            Decision::Act { tool, remaining, .. } => {
                assert_eq!(tool, "echo");
                assert!(remaining.contains("calc 2+2"));
                assert!(remaining.contains("收尾"));
            }
            _ => panic!("expected Act"),
        }
    }

    #[tokio::test]
    async fn local_reasoner_multiline_args_merged() {
        let mut known = HashSet::new();
        known.insert("file_write".to_string());
        // @tool 行之后的非 @ 内容并入参数（多行内容）
        let d = LocalReasoner
            .decide("@file_write a.txt\nhello", &known)
            .await
            .unwrap();
        match d {
            Decision::Act { tool, args, remaining } => {
                assert_eq!(tool, "file_write");
                assert_eq!(args, "a.txt\nhello");
                assert!(remaining.is_empty());
            }
            _ => panic!("expected Act"),
        }
    }

    #[tokio::test]
    async fn local_reasoner_json_call_resolves() {
        let mut known = HashSet::new();
        known.insert("file_write".to_string());
        let d = LocalReasoner
            .decide(r#"{"tool":"file_write","args":"a.txt\nhello"}"#, &known)
            .await
            .unwrap();
        match d {
            Decision::Act { tool, args, remaining } => {
                assert_eq!(tool, "file_write");
                assert_eq!(args, "a.txt\nhello");
                assert!(remaining.is_empty());
            }
            _ => panic!("expected Act"),
        }
    }

    #[tokio::test]
    async fn local_reasoner_final_when_no_tool() {
        let known = HashSet::new();
        let d = LocalReasoner.decide("你好", &known).await.unwrap();
        assert!(matches!(d, Decision::Final(_)));
    }
}
