//! 工程化配置层：集中管理 `GANYU_*` 环境变量（单一事实来源）。
//!
//! 对标 Pi「配置即文件、可版本管理」与 OpenClaw「config JSON 缓存复用」：
//! ganyu 保持零依赖，用统一命名空间的环境变量做配置，本模块提供：
//! - 类型化读取（TTL 毫秒、速率、布尔开关）；
//! - `security_baseline()`：对当前配置做安全基线自检（治理面），
//!   如「shell 已开但未启用 sandbox/容器」给出告警建议。
//!
//! 说明：既有 `security.rs` / `memory.rs` 的读取点是**执行面**（失败闭环），保持原位不动；
//! 本模块是**配置面**的权威文档与新增能力（缓存/审计/限速）的入口。两处共用同一 env 命名空间。

use std::time::Duration;

/// 全部 `GANYU_*` 环境变量及其含义（文档面，供 README/SECURITY 引用）。
pub const ENV_DOCS: &[(&str, &str)] = &[
    ("GANYU_FS_ROOT", "文件沙箱根目录（默认 .ganyu_workspace）"),
    ("GANYU_MEM_KEY", "记忆加密 passphrase（crypto 特性下生效）"),
    ("GANYU_ALLOW_SHELL", "=1 时放行 exec（需 shell 特性编译）"),
    ("GANYU_ALLOW_PLUGINS", "=1 时启用插件发现（C2）"),
    ("GANYU_PLUGIN_ALLOW", "插件程序名白名单（逗号分隔）"),
    ("GANYU_TOOL_CACHE_TTL", "只读工具结果缓存 TTL（毫秒，>0 启用；默认 0=关）"),
    ("GANYU_LLM_CACHE_TTL", "LLM 响应缓存 TTL（毫秒，>0 启用；默认 0=关）"),
    ("GANYU_RATE_PER_MIN", "网关请求速率上限（每分钟；默认 0=不限）"),
    ("GANYU_AUDIT", "审计日志：1=stderr，或文件路径（默认 0=关）"),
    ("OV_BASE", "OpenViking 记忆服务地址（network 特性下生效）"),
    ("OPENAI_API_BASE / OPENAI_API_KEY", "OpenAI 兼容后端（network 特性下生效）"),
    ("OPENAI_MODEL", "模型 id（默认 gpt-4o-mini；推理模型自动兼容 reasoning_content）"),
    ("GANYU_CONFIG", "配置文件路径（默认 ~/.ganyu/config.toml）"),
];

/// 从配置文件加载模型配置（对标 OpenClaw `config.yaml` / Hermes 配置文件），
/// 实现「一站式」：写一次 `~/.ganyu/config.toml`，之后直接 `ganyu-agent chat` 即可对话。
///
/// 规则：
/// - 路径优先级：`$GANYU_CONFIG` > `~/.ganyu/config.toml` > `./ganyu.toml`；
/// - **已设置的环境变量优先于文件**（env 覆盖文件，便于 CI/容器注入）；
/// - 文件格式（toml）：
///   ```toml
///   [model]
///   base_url = "https://apihub.agnes-ai.com/v1"
///   api_key = "sk-..."
///   model = "agnes-2.5-flash"
///   ```
pub fn load_model_config() {
    let path = std::env::var("GANYU_CONFIG").ok().or_else(|| {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok()
            .map(|h| format!("{h}/.ganyu/config.toml"))
            .or_else(|| Some("ganyu.toml".to_string()))
    });
    let Some(path) = path else { return };
    let Ok(text) = std::fs::read_to_string(&path) else { return };

    #[derive(serde::Deserialize, Default)]
    struct FileCfg {
        #[serde(default)]
        model: Option<ModelCfg>,
    }
    #[derive(serde::Deserialize)]
    struct ModelCfg {
        base_url: Option<String>,
        api_key: Option<String>,
        model: Option<String>,
    }
    let Ok(parsed) = toml::from_str::<FileCfg>(&text) else { return };
    let Some(m) = parsed.model else { return };

    if std::env::var("OPENAI_API_BASE").is_err() {
        if let Some(b) = m.base_url {
            std::env::set_var("OPENAI_API_BASE", b);
        }
    }
    if std::env::var("OPENAI_API_KEY").is_err() {
        if let Some(k) = m.api_key {
            std::env::set_var("OPENAI_API_KEY", k);
        }
    }
    if std::env::var("OPENAI_MODEL").is_err() {
        if let Some(md) = m.model {
            std::env::set_var("OPENAI_MODEL", md);
        }
    }
}

/// 配置快照（启动时读取一次）。
#[derive(Debug, Clone)]
pub struct GanyuConfig {
    pub fs_root: String,
    pub tool_cache_ttl: Duration,
    pub llm_cache_ttl: Duration,
    pub rate_per_min: u32,
    pub audit: AuditTarget,
    pub shell_allowed: bool,
    pub plugins_allowed: bool,
}

/// 审计目标：关闭 / stderr / 文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditTarget {
    Off,
    Stderr,
    File(String),
}

impl GanyuConfig {
    pub fn from_env() -> Self {
        GanyuConfig {
            fs_root: std::env::var("GANYU_FS_ROOT")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| ".ganyu_workspace".to_string()),
            tool_cache_ttl: env_ttl("GANYU_TOOL_CACHE_TTL"),
            llm_cache_ttl: env_ttl("GANYU_LLM_CACHE_TTL"),
            rate_per_min: env_u32("GANYU_RATE_PER_MIN").unwrap_or(0),
            audit: match std::env::var("GANYU_AUDIT") {
                Ok(v) if v == "1" => AuditTarget::Stderr,
                Ok(v) if v.eq_ignore_ascii_case("stderr") => AuditTarget::Stderr,
                Ok(v) if !v.is_empty() => AuditTarget::File(v),
                _ => AuditTarget::Off,
            },
            shell_allowed: std::env::var("GANYU_ALLOW_SHELL").as_deref() == Ok("1"),
            plugins_allowed: std::env::var("GANYU_ALLOW_PLUGINS").as_deref() == Ok("1"),
        }
    }

    /// 只读工具缓存是否启用。
    pub fn tool_cache_enabled(&self) -> bool {
        self.tool_cache_ttl > Duration::ZERO
    }

    /// LLM 缓存是否启用。
    pub fn llm_cache_enabled(&self) -> bool {
        self.llm_cache_ttl > Duration::ZERO
    }
}

/// 安全基线自检（治理面）：返回建议列表（空=无建议）。
/// 不阻断运行，仅给出生产部署前的告警——与 Hermes「8 层防线、逐步放权」一致。
pub fn security_baseline(cfg: &GanyuConfig) -> Vec<String> {
    let mut advice = Vec::new();
    if cfg.shell_allowed && !cfg::sandbox_available() {
        advice.push(
            "GANYU_ALLOW_SHELL=1 但未开启 sandbox 特性/容器隔离：exec 将以进程权限直跑，\
             生产建议用 Docker/gVisor 或开启 sandbox(Landlock, Linux)。".into(),
        );
    }
    if cfg.plugins_allowed {
        let allow = std::env::var("GANYU_PLUGIN_ALLOW").unwrap_or_default();
        if allow.is_empty() {
            advice.push("GANYU_ALLOW_PLUGINS=1 但 GANYU_PLUGIN_ALLOW 为空：插件将被全部拒绝（安全，但无用）。".into());
        }
    }
    if std::env::var("GANYU_MEM_KEY").is_ok() {
        let key = std::env::var("GANYU_MEM_KEY").unwrap_or_default();
        if key.len() < 12 {
            advice.push("GANYU_MEM_KEY 过短（<12 字符）：记忆加密强度不足，建议 ≥16 字符强口令。".into());
        }
    }
    if cfg.rate_per_min == 0 && cfg.llm_cache_enabled() {
        advice.push("已启用 LLM 缓存但未设 GANYU_RATE_PER_MIN：建议同时限速以防突发流量。".into());
    }
    advice
}

/// 解析 `N`（毫秒）为 `Duration`；非法/缺省返回 `Duration::ZERO`（=关闭）。
pub fn ttl_from_str(s: &str) -> Duration {
    Duration::from_millis(s.trim().parse::<u32>().unwrap_or(0).into())
}

fn env_ttl(name: &str) -> Duration {
    std::env::var(name).ok().map(|s| ttl_from_str(&s)).unwrap_or_else(|| Duration::ZERO)
}

fn env_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok().and_then(|s| s.trim().parse::<u32>().ok())
}

/// sandbox 是否可用（当前编译目标 + 特性）。
mod cfg {
    pub fn sandbox_available() -> bool {
        #[cfg(all(feature = "sandbox", target_os = "linux"))]
        {
            true
        }
        #[cfg(not(all(feature = "sandbox", target_os = "linux")))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_fail_closed() {
        let c = GanyuConfig::from_env();
        // 默认：缓存关、限速 0、shell 关、插件关。
        assert!(!c.tool_cache_enabled());
        assert!(!c.llm_cache_enabled());
        assert_eq!(c.rate_per_min, 0);
        assert!(!c.shell_allowed);
        assert!(!c.plugins_allowed);
    }

    #[test]
    fn ttl_parses_ms_without_env() {
        assert_eq!(ttl_from_str("500").as_millis(), 500);
        assert_eq!(ttl_from_str("0").as_millis(), 0);
        assert_eq!(ttl_from_str("abc").as_millis(), 0);
    }
}
