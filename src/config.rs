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

use std::path::Path;
use std::time::Duration;

use crate::GanyuError;

/// 配置文件路径（与 `load_model_config` 同优先级：`$GANYU_CONFIG` > `~/.ganyu/config.toml` > `./ganyu.toml`）。
pub fn config_path() -> Option<String> {
    std::env::var("GANYU_CONFIG").ok().or_else(|| {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok()
            .map(|h| format!("{h}/.ganyu/config.toml"))
            .or_else(|| Some("ganyu.toml".to_string()))
    })
}

/// 当前配置文件中的 [model] 段（供 `setup` / `model` 显示现状）。
pub fn read_model_config() -> (Option<String>, Option<String>, Option<String>) {
    let path = config_path();
    let Some(path) = path else { return (None, None, None) };
    let Ok(text) = std::fs::read_to_string(&path) else { return (None, None, None) };
    #[derive(serde::Deserialize)]
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
    let Ok(parsed) = toml::from_str::<FileCfg>(&text) else { return (None, None, None) };
    let Some(m) = parsed.model else { return (None, None, None) };
    (m.base_url, m.api_key, m.model)
}

/// 写入 [model] 段（`ganyu setup` 用）。保留文件中其他段，创建父目录。
pub fn write_model_config(base_url: &str, api_key: &str, model: &str) -> crate::GanyuResult<()> {
    let path = config_path().ok_or_else(|| {
        GanyuError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "无法确定配置文件路径（GANYU_CONFIG/USERPROFILE/HOME 均未设置）",
        ))
    })?;
    let mut value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
        .unwrap_or_else(|| toml::Value::Table(Default::default()));
    let mut model_tbl = toml::map::Map::new();
    model_tbl.insert("base_url".to_string(), toml::Value::String(base_url.to_string()));
    model_tbl.insert("api_key".to_string(), toml::Value::String(api_key.to_string()));
    model_tbl.insert("model".to_string(), toml::Value::String(model.to_string()));
    if let toml::Value::Table(map) = &mut value {
        map.insert("model".to_string(), toml::Value::Table(model_tbl));
    }
    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(&value).map_err(|e| GanyuError::Toml(e.to_string()))?;
    std::fs::write(&path, text)?;
    // F-01：配置文件含 API Key，写后收紧为属主只读（Unix 0600）。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(mut perms) = std::fs::metadata(&path).map(|m| m.permissions()) {
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(&path, perms);
        }
    }
    Ok(())
}

/// 读取 [gateway] 段的 Telegram bot token（`ganyu gateway start` 用）。
pub fn read_gateway_token() -> Option<String> {
    let path = config_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    #[derive(serde::Deserialize)]
    struct FileCfg {
        #[serde(default)]
        gateway: Option<GatewayCfg>,
    }
    #[derive(serde::Deserialize)]
    struct GatewayCfg {
        telegram_token: Option<String>,
    }
    let parsed = toml::from_str::<FileCfg>(&text).ok()?;
    parsed.gateway?.telegram_token
}

/// 写入 [gateway] 段（`ganyu gateway setup` 用）。保留其他段。
pub fn write_gateway_token(token: &str) -> crate::GanyuResult<()> {
    let path = config_path().ok_or_else(|| {
        GanyuError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "无法确定配置文件路径（GANYU_CONFIG/USERPROFILE/HOME 均未设置）",
        ))
    })?;
    let mut value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
        .unwrap_or_else(|| toml::Value::Table(Default::default()));
    let mut gw_tbl = toml::map::Map::new();
    gw_tbl.insert("telegram_token".to_string(), toml::Value::String(token.to_string()));
    if let toml::Value::Table(map) = &mut value {
        map.insert("gateway".to_string(), toml::Value::Table(gw_tbl));
    }
    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(&value).map_err(|e| GanyuError::Toml(e.to_string()))?;
    std::fs::write(&path, text)?;
    Ok(())
}

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

/// 配置自愈：配置文件不存在时自动生成可编辑模板（**不含密钥**）。
///
/// 解决根因：`~/.ganyu` 被外部清理后，配置会静默丢失导致"即开即用失效"。
/// 启动时调用本函数，目录被删后首次运行即自动重建模板，用户只需填 key
/// （或直接运行 `ganyu-agent setup`）。
///
/// 返回生成的文件路径；文件已存在或生成失败返回 `None`。
pub fn ensure_config_template() -> Option<String> {
    let path = config_path()?;
    if std::path::Path::new(&path).exists() {
        return None;
    }
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmpl = "# ganyu-agent 配置模板（自动生成；填好即可对话，或运行 ganyu-agent setup）\n\
                [model]\n\
                base_url = \"https://api.openai.com/v1\"\n\
                api_key = \"\"\n\
                model = \"gpt-4o-mini\"\n";
    if std::fs::write(&path, tmpl).is_ok() {
        Some(path)
    } else {
        None
    }
}

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
    if std::env::var("OPENAI_MODEL").is_err() {
        if let Some(md) = m.model {
            std::env::set_var("OPENAI_MODEL", md);
        }
    }
    // F-10：API Key 不再注入进程全局环境变量（避免泄漏到子进程 / 插件环境）。
    // 调用方应改用 `read_model_config()` 显式取用密钥。
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
    use std::sync::Mutex;

    /// env 隔离：所有测试串行执行（GANYU_CONFIG 是进程级全局，并行会互相覆盖）。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn tmp_config(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ganyu-cfg-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.toml")
    }

    #[test]
    fn write_read_model_roundtrip() {
        let _g = ENV_LOCK.lock().unwrap();
        let p = tmp_config("model");
        std::env::set_var("GANYU_CONFIG", &p);
        write_model_config("https://api.test/v1", "sk-test-key-123456", "model-1").unwrap();
        let (b, k, m) = read_model_config();
        assert_eq!(b.as_deref(), Some("https://api.test/v1"));
        assert_eq!(k.as_deref(), Some("sk-test-key-123456"));
        assert_eq!(m.as_deref(), Some("model-1"));
        std::env::remove_var("GANYU_CONFIG");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn write_preserves_other_sections() {
        let _g = ENV_LOCK.lock().unwrap();
        let p = tmp_config("preserve");
        std::env::set_var("GANYU_CONFIG", &p);
        // 预置文件含其他段
        std::fs::write(
            &p,
            "[other]\nkey = \"keep-me\"\n",
        )
        .unwrap();
        write_model_config("https://api.test/v1", "sk-k", "m").unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("keep-me"), "其他段被覆盖: {text}");
        assert!(text.contains("[model]"));
        std::env::remove_var("GANYU_CONFIG");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn gateway_token_roundtrip_with_model_coexist() {
        let _g = ENV_LOCK.lock().unwrap();
        let p = tmp_config("gw");
        std::env::set_var("GANYU_CONFIG", &p);
        write_gateway_token("123456:ABC-DEF").unwrap();
        assert_eq!(read_gateway_token().as_deref(), Some("123456:ABC-DEF"));
        // model 与 gateway 段共存
        write_model_config("https://api.test/v1", "sk-k", "m").unwrap();
        assert_eq!(read_gateway_token().as_deref(), Some("123456:ABC-DEF"));
        let (_, _, m) = read_model_config();
        assert_eq!(m.as_deref(), Some("m"));
        std::env::remove_var("GANYU_CONFIG");
        let _ = std::fs::remove_file(&p);
    }

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
