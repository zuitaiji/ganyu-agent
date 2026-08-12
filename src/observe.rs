//! 可观测性/审计日志（治理面）：结构化记录工具调用、安全拒绝、网关级联、限速与缓存命中。
//!
//! 对标：
//! - Pi 的 tamper-evidence ledger（每次工具执行写可追溯风险记录）；
//! - OpenClaw 的「发布日志 + artifact 留证」；
//! - Hermes 的「先测试 logs / approvals / shutdown path」。
//!
//! 设计：轻量、零依赖——JSON Lines 输出到 stderr 或文件（`GANYU_AUDIT=1|stderr|<path>`）。
//! 默认关闭；开启后每个事件一行 JSON，便于用 jq / 日志系统收集做合规审计与故障排查。

use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::AuditTarget;

/// 审计事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditEvent<'a> {
    ToolCall { tool: &'a str, ok: bool, ms: u64 },
    ToolCacheHit { tool: &'a str },
    SecurityDenial { kind: &'a str, reason: &'a str },
    GatewayFallback { from: &'a str, to: &'a str },
    RateLimited { reason: &'a str },
    LlmCacheHit { ms: u64 },
    BaselineAdvice { advice: &'a str },
}

/// 审计日志句柄（进程内单例，线程安全）。
pub struct AuditLog {
    target: AuditTarget,
    writer: Mutex<Option<std::fs::File>>,
}

impl AuditLog {
    pub fn new(target: AuditTarget) -> Self {
        let writer = match &target {
            AuditTarget::File(path) => std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
                .map(|f| std::fs::File::try_clone(&f).unwrap_or(f)),
            _ => None,
        };
        AuditLog { target, writer: Mutex::new(writer) }
    }

    pub fn from_config() -> Self {
        AuditLog::new(crate::config::GanyuConfig::from_env().audit)
    }

    pub fn is_enabled(&self) -> bool {
        self.target != AuditTarget::Off
    }

    /// 记录一个事件（JSON Lines）。
    pub fn event(&self, event: AuditEvent) {
        if !self.is_enabled() {
            return;
        }
        let (kind, detail) = self.render(event);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let line = format!(
            "{{\"ts\":{ts:.3},\"ev\":\"{kind}\",\"detail\":{detail}}}\n"
        );
        match &self.target {
            AuditTarget::Stderr => {
                let _ = eprint!("{line}");
            }
            AuditTarget::File(_) => {
                if let Some(f) = self.writer.lock().unwrap().as_mut() {
                    let _ = f.write_all(line.as_bytes());
                    let _ = f.flush();
                }
            }
            AuditTarget::Off => {}
        }
    }

    fn render(&self, e: AuditEvent) -> (String, String) {
        let detail = |s: &str| -> String { serde_json::json!({ "m": s }).to_string() };
        let kv = |k: &str, v: &str| -> String { serde_json::json!({ "k": k, "v": v }).to_string() };
        match e {
            AuditEvent::ToolCall { tool, ok, ms } => (
                "tool_call".into(),
                serde_json::json!({ "tool": tool, "ok": ok, "ms": ms }).to_string(),
            ),
            AuditEvent::ToolCacheHit { tool } => ("tool_cache_hit".into(), detail(tool)),
            AuditEvent::SecurityDenial { kind, reason } => {
                ("security_denial".into(), kv(kind, reason))
            }
            AuditEvent::GatewayFallback { from, to } => {
                ("gateway_fallback".into(), kv(from, to))
            }
            AuditEvent::RateLimited { reason } => ("rate_limited".into(), detail(reason)),
            AuditEvent::LlmCacheHit { ms } => {
                ("llm_cache_hit".into(), serde_json::json!({ "ms": ms }).to_string())
            }
            AuditEvent::BaselineAdvice { advice } => ("baseline_advice".into(), detail(advice)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_produces_json_lines() {
        let log = AuditLog::new(AuditTarget::Off);
        let (kind, detail) = log.render(AuditEvent::ToolCall { tool: "calc", ok: true, ms: 3 });
        assert_eq!(kind, "tool_call");
        assert!(detail.contains("\"tool\":\"calc\""));
        assert!(detail.contains("\"ok\":true"));
    }
}
