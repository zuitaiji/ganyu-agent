//! ganyu-agent CLI。
//!
//! 子命令：
//! - `run "查询"`  ：跑完整 ReAct 推理循环（多步工具调用），打印轨迹与最终作答。
//! - `tools`        ：列出全部内置工具与特性技能。
//! - `skill <名> <参数>`：直接调用某个特性技能（如 `skill summarize path`）。
//! - `sag "问题"`   ：跑知识/分析面的 SAG 管道（默认 examples/sample_mdl.json）。
//! - `selftest`     ：运行内置自愈/可拓展自检（无需 cargo test）。
//! - `chat`         ：读 stdin 一行，经 Agent 推理循环响应。

use std::path::Path;
use std::sync::Arc;

use ganyu_agent::core::llm::{DynBackend, LlmBackend, LocalBackend, Message};
#[cfg(feature = "network")]
use ganyu_agent::core::llm::OpenAiBackend;
use ganyu_agent::core::loop_::{LlmReasoner, LocalReasoner, Step};
use ganyu_agent::core::memory::{DynMemory, LocalMemory};
use ganyu_agent::core::unit::{RunContext, Unit};
use ganyu_agent::core::workflow::{
    BlackboardWorkflow, GraphBuilder, KeywordRouter, LocalPlanner, MultiAgentWorkflow,
    PlanExecuteWorkflow, RouterWorkflow, SingleWorkflow, Workflow,
};
use ganyu_agent::core::Agent;
use ganyu_agent::ext::builtins::register_core_tools;
use ganyu_agent::ext::nomifun_caps::{register_nomifun_skills, NomifunSkillTool};
use ganyu_agent::ext::skills::{register_core_skills, SkillTool};
use ganyu_agent::ext::SkillBook;
use ganyu_agent::ext::ToolRegistry;
use ganyu_agent::heal::with_retry;
use ganyu_agent::knowledge::mdl::Mdl;
use ganyu_agent::knowledge::sag::{SagPipeline, Verdict};
use ganyu_agent::routing::Gateway;
use ganyu_agent::session::SessionId;
use ganyu_agent::value::Value;
use ganyu_agent::{GanyuError, GanyuResult};
use std::collections::HashMap;

/// 计算文件 SHA256（update 校验用）。用系统工具避免新增依赖：
/// Windows `certutil -hashfile`，Linux/macOS `sha256sum`。
fn sha256_of_file(path: &Path) -> GanyuResult<String> {
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("certutil")
            .args(["-hashfile", path.to_str().ok_or_else(|| GanyuError::Http("文件路径包含非 UTF-8 字符，无法计算 SHA256".to_string()))?, "SHA256"])
            .output()
            .map_err(GanyuError::Io)?;
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let t = line.trim();
            if t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(t.to_lowercase());
            }
        }
        Err(GanyuError::Http("certutil 输出解析失败（未找到 SHA256 行）".to_string()))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let out = std::process::Command::new("sha256sum")
            .arg(path)
            .output()
            .map_err(GanyuError::Io)?;
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(text.split_whitespace().next().unwrap_or("").to_lowercase())
    }
}

fn default_memory_path() -> std::path::PathBuf {
    let base = std::env::var("GANYU_HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let mut p = std::path::PathBuf::from(base);
    p.push(".ganyu");
    p.push("ganyu_agent_memory.json");
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    p
}

#[tokio::main]
async fn main() -> GanyuResult<()> {
    let raw: Vec<String> = std::env::args().collect();
    let mut session = SessionId::new();
    let mut cmd: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut mode_arg: Option<String> = None;
    let mut k = 1;
    while k < raw.len() {
        if raw[k] == "--session" {
            if let Some(s) = raw.get(k + 1) {
                if let Ok(u) = uuid::Uuid::parse_str(s) {
                    session = SessionId(u);
                }
            }
            k += 2;
            continue;
        }
        if raw[k] == "--mode" {
            if let Some(m) = raw.get(k + 1) {
                mode_arg = Some(m.clone());
            }
            k += 2;
            continue;
        }
        if matches!(raw[k].as_str(), "--version" | "-V") {
            println!("ganyu-agent {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        if matches!(raw[k].as_str(), "--help" | "-h" | "help") {
            println!("ganyu-agent — 有温度、能自进化、可拓展、可自愈的 agent 系统");
            println!();
            println!("用法: ganyu [子命令] [参数]");
            println!();
            println!("  对话      chat             交互式 REPL（默认，无参数即进入）");
            println!("  配置      setup            交互式配置模型（base_url/api_key/model）");
            println!("           model [新模型id]   查看/切换模型");
            println!("           models            查询网关可用模型列表");
            println!("           doctor            环境诊断");
            println!("  执行      run \"<脚本>\"      ReAct 多步推理");
            println!("           agent \"任务\" --mode <范式>   指定范式编排");
            println!("           sag \"<问题>\"      知识分析（SAG 管道）");
            println!("           skill <名> <参数>  直接调用技能");
            println!("  运维      update            从 GitHub Releases 自更新");
            println!("           gateway setup/start   Telegram 消息平台网关");
            println!("           tools | modes | selftest   工具/范式/自检");
            println!();
            println!("完整文档: https://github.com/zuitaiji/ganyu-agent/blob/main/docs/usage.md");
            return Ok(());
        }
        if cmd.is_none() {
            cmd = Some(raw[k].clone());
            k += 1;
            continue;
        }
        positional.push(raw[k].clone());
        k += 1;
    }
    let cmd = cmd.as_deref().unwrap_or("chat");

    let memory: DynMemory = Arc::new(LocalMemory::new(default_memory_path()));

    // 工程化配置面：集中读取 GANYU_*，据此启用缓存/限速/审计。
    let cfg = ganyu_agent::config::GanyuConfig::from_env();
    // 一站式：从 ~/.ganyu/config.toml 加载模型配置（已设置的环境变量优先）。
    // 凭据改为下方 read_model_config 显式取用（F-10），不再全局 set_var。
    let audit = Arc::new(ganyu_agent::observe::AuditLog::from_config());

    let mut gateway = Gateway::new();
    gateway.register(Arc::new(LocalBackend) as DynBackend);
    if cfg.rate_per_min > 0 {
        gateway = gateway.with_rate_limit(cfg.rate_per_min);
    }
    if cfg.llm_cache_enabled() {
        gateway.enable_llm_cache(cfg.llm_cache_ttl);
    }
    gateway.set_audit(audit.clone());

    // F-10：凭据优先取环境变量，回退到配置文件（read_model_config），不再依赖 load_model_config 写入全局环境。
    let mcfg = ganyu_agent::config::read_model_config();
    let base = std::env::var("OPENAI_API_BASE").ok().or(mcfg.0);
    let key = std::env::var("OPENAI_API_KEY").ok().or(mcfg.1);
    let model = std::env::var("OPENAI_MODEL").ok().or(mcfg.2);
    if let (Some(base), Some(key)) = (base, key) {
        // 模型名可用 OPENAI_MODEL 覆盖（默认 gpt-4o-mini；OpenAI 兼容端点均可）。
        let model = model.unwrap_or_else(|| "gpt-4o-mini".to_string());
        #[cfg(feature = "network")]
        gateway.register(Arc::new(OpenAiBackend::new(&base, &key, &model)) as DynBackend);
        #[cfg(not(feature = "network"))]
        let _ = (base, key, model);
    }

    // 工具层：内置能力 + 插件发现 + 只读缓存
    let tools = Arc::new(ToolRegistry::new());
    register_core_tools(&tools, memory.clone());
    if cfg.tool_cache_enabled() {
        tools.enable_tool_cache(cfg.tool_cache_ttl);
    }
    tools.set_audit(audit.clone());
    let _ = tools.discover(Path::new("plugins"));

    // 安全基线自检（治理面）：启动时输出建议，不阻断。
    for advice in ganyu_agent::config::security_baseline(&cfg) {
        eprintln!("[baseline] {advice}");
        audit.event(ganyu_agent::observe::AuditEvent::BaselineAdvice { advice: &advice });
    }

    // 技能层：内置特性技能 + 注册为 `skill:<name>` 工具
    let skills = Arc::new(SkillBook::new(memory.clone()));
    register_core_skills(&skills);
    register_nomifun_skills(&skills); // nomifun 内置 agent 能力全量赋能
    tools.register(Arc::new(NomifunSkillTool)); // nomifun 能力派发（离线/网关桥接）
    for name in skills.skill_names() {
        let desc = skills
            .get_skill(&name)
            .map(|s| s.description.clone())
            .unwrap_or_default();
        tools.register(Arc::new(SkillTool::new(
            skills.clone(),
            tools.clone(),
            name.clone(),
            desc,
        )));
    }

    // 推理器：配置了 OpenAI 兼容模型（network 特性）→ 用 LlmReasoner 接真模型；否则离线 LocalReasoner。
    let gateway_arc = Arc::new(gateway);
    #[cfg(feature = "network")]
    let reasoner: Arc<dyn ganyu_agent::core::loop_::Reasoner> = {
        // F-10：API Key 不注入全局 env（防泄漏子进程），key 从 env 或 config.toml 取。
        let key_ok = std::env::var("OPENAI_API_KEY").is_ok()
            || ganyu_agent::config::read_model_config().1.is_some();
        let has_model = std::env::var("OPENAI_API_BASE").is_ok() && key_ok;
        if has_model {
            Arc::new(LlmReasoner::new(gateway_arc.clone()))
        } else {
            Arc::new(LocalReasoner)
        }
    };
    #[cfg(not(feature = "network"))]
    let reasoner: Arc<dyn ganyu_agent::core::loop_::Reasoner> = Arc::new(LocalReasoner);

    let agent = ganyu_agent::core::Agent::new(
        gateway_arc,
        memory.clone(),
        tools.clone(),
        skills.clone(),
        reasoner,
        session,
    );

    match cmd {
        "run" => {
            let query = positional.join(" ");
            println!("session: {session}");
            let out = agent.run(&Value(query)).await?;
            print_trace(&agent);
            println!("\n>> {}", out);
        }
        "tools" => {
            println!("== 工具 ==");
            for n in tools.names() {
                if let Some(t) = tools.get_description(&n) {
                    println!("  {n} - {t}");
                } else {
                    println!("  {n}");
                }
            }
            println!("\n== 特性技能 ==");
            for s in skills.skill_specs() {
                println!("  {s}");
            }
        }
        "skill" => {
            let name = positional.first().cloned().unwrap_or_default();
            let args = positional[1..].join(" ");
            println!("session: {session}");
            let out = tools
                .call(&format!("skill:{name}"), &Value(args))
                .await?;
            println!("{out}");
        }
        "sag" => {
            let query = positional
                .first()
                .cloned()
                .unwrap_or_else(|| "上月华东区利润最高的三个产品".to_string());
            let mdl = Mdl::load("examples/sample_mdl.json")?;
            let pipeline = SagPipeline {
                mdl: Arc::new(mdl),
                gateway: agent.gateway.clone(),
                memory: memory.clone(),
                skills: agent.skills.clone(),
                session,
            };
            let out = pipeline.run(&Value(query), None).await?;
            println!("session: {session}");
            println!("verdict: {:?}", out.verdict);
            println!("sql:\n{}", out.sql);
            if let Some(r) = out.result {
                println!("rows: {r}");
            }
        }
        "selftest" => {
            selftest().await;
        }
        "modes" => {
            println!("支持的 agent 范式（--mode）：");
            for (m, desc) in [
                ("single", "单 agent（Unit 直跑）"),
                ("react", "ReAct 多步推理循环（默认单 agent 内部行为）"),
                ("plan", "Plan & Execute：先规划再逐步执行"),
                ("multi", "多 agent 协作：按轮次传递上下文"),
                ("router", "Router：分类派发到专精 agent/skill"),
                ("blackboard", "Blackboard：共享黑板 + 合成器"),
                ("graph", "Graph Workflow：DAG 拓扑执行"),
            ] {
                println!("  {m:11} - {desc}");
            }
        }
        "doctor" => {
            // 环境/配置诊断（对标 OpenClaw 开箱自检）：快速定位「为什么没接上模型」。
            println!("== ganyu-agent 环境诊断 ==");
            println!(
                "编译特性 : network={} crypto={} secret={} shell={} sandbox={}",
                cfg!(feature = "network"),
                cfg!(feature = "crypto"),
                cfg!(feature = "secret"),
                cfg!(feature = "shell"),
                cfg!(feature = "sandbox"),
            );
            let cfg_path = std::env::var("GANYU_CONFIG")
                .ok()
                .or_else(|| {
                    std::env::var("USERPROFILE")
                        .or_else(|_| std::env::var("HOME"))
                        .ok()
                        .map(|h| format!("{h}/.ganyu/config.toml"))
                })
                .unwrap_or_else(|| "ganyu.toml".to_string());
            println!(
                "配置文件 : {cfg_path} [{}]",
                if std::path::Path::new(&cfg_path).exists() {
                    "存在"
                } else {
                    "缺失（可写 [model] 段一键接入模型）"
                }
            );
            let base = std::env::var("OPENAI_API_BASE").unwrap_or_default();
            // F-10：key 不注入全局 env，从 env 或 config.toml 取
            let key_set = std::env::var("OPENAI_API_KEY").is_ok()
                || ganyu_agent::config::read_model_config().1.is_some();
            let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini(默认)".into());
            println!(
                "模型配置 : base={} key={} model={}",
                if base.is_empty() { "(未设置)" } else { &base },
                if key_set { "已设置" } else { "未设置" },
                model,
            );
            println!("网关后端 : {}", agent.gateway.names().join(", "));
            println!(
                "能力面   : 工具 {} 个；技能 {} 个",
                tools.names().len(),
                skills.skill_names().len()
            );
            let mem_path = default_memory_path();
            let mem_ok = mem_path.exists();
            println!(
                "记忆文件 : {} [{}]",
                mem_path.display(),
                if mem_ok { "存在" } else { "尚未创建" }
            );
            let model_ready = cfg!(feature = "network")
                && !base.is_empty()
                && key_set;
            println!(
                "状态     : {}",
                if model_ready {
                    "✅ 模型已配置，直接 ganyu-agent chat 即可对话"
                } else if cfg!(feature = "network") {
                    "⚠️ 未配置模型：编辑上面的 config.toml（[model] base_url/api_key/model）后即可对话"
                } else {
                    "⚠️ 当前构建无 network 特性（离线模式）；用 --features hardened 编译以接入模型"
                }
            );
        }
        "models" => {
            // 查询 OpenAI 兼容网关的可用模型列表（GET /v1/models）。
            #[cfg(feature = "network")]
            {
                let base = std::env::var("OPENAI_API_BASE").unwrap_or_default();
                let key = std::env::var("OPENAI_API_KEY")
                    .ok()
                    .or_else(|| ganyu_agent::config::read_model_config().1)
                    .unwrap_or_default();
                if base.is_empty() || key.is_empty() {
                    eprintln!("未配置模型端点：编辑 ~/.ganyu/config.toml 的 [model] 段，或运行 doctor 查看。");
                    std::process::exit(1);
                }
                let backend = OpenAiBackend::new(&base, &key, "models-probe");
                match backend.list_models().await {
                    Ok(list) => {
                        println!("== 可用模型（{} 个）==", list.len());
                        for m in list {
                            println!("  {m}");
                        }
                    }
                    Err(e) => {
                        eprintln!("查询失败: {e}");
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(feature = "network"))]
            {
                eprintln!("当前构建无 network 特性，无法查询模型列表。请用 --features network/hardened 编译。");
                std::process::exit(1);
            }
        }
        "setup" => {
            // 交互式配置向导（Hermes 式）：逐步提问 base_url/api_key/model 写入 config.toml。
            // 也支持参数模式：ganyu setup --base_url X --api_key Y --model Z（脚本/CI 用）。
            use std::io::{IsTerminal, Write as _};

            let (cur_base, cur_key, cur_model) = ganyu_agent::config::read_model_config();
            let mask = |s: &Option<String>| {
                s.as_deref().map(|v| {
                    let chars: Vec<char> = v.chars().collect();
                    if chars.len() > 8 {
                        format!(
                            "{}…{}",
                            chars[..4].iter().collect::<String>(),
                            chars[chars.len() - 4..].iter().collect::<String>()
                        )
                    } else {
                        "****".to_string()
                    }
                })
            };

            // 参数模式：解析 positional 中的 --key value
            let mut arg_base: Option<String> = None;
            let mut arg_key: Option<String> = None;
            let mut arg_model: Option<String> = None;
            let mut it = positional.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--base_url" => arg_base = it.next().cloned(),
                    "--api_key" => arg_key = it.next().cloned(),
                    "--model" => arg_model = it.next().cloned(),
                    _ => {}
                }
            }

            let ask = |label: &str, cur: Option<String>, is_secret: bool| -> std::io::Result<String> {
                let hint = if is_secret {
                    mask(&cur).map(|m| format!(" [{m}]")).unwrap_or_default()
                } else {
                    cur.as_deref().map(|c| format!(" [{c}]")).unwrap_or_default()
                };
                print!("{label}{hint}: ");
                std::io::stdout().flush()?;
                // 密钥用掩码输入（rpassword，终端不回显明文）
                let v = if is_secret {
                    rpassword::read_password()?.trim().to_string()
                } else {
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line.trim().to_string()
                };
                if v.is_empty() {
                    Ok(cur.unwrap_or_default())
                } else {
                    Ok(v)
                }
            };

            let interactive = std::io::stdin().is_terminal();
            let (base, key, model) = if interactive && arg_base.is_none() && arg_key.is_none() && arg_model.is_none() {
                println!("== ganyu setup（配置模型，回车沿用当前值）==");
                let base = ask("base_url", cur_base.clone(), false)?;
                let key = ask("api_key", cur_key.clone(), true)?;
                let model = ask("model", cur_model.clone(), false)?;
                (base, key, model)
            } else {
                (
                    arg_base.or(cur_base).unwrap_or_default(),
                    arg_key.or(cur_key).unwrap_or_default(),
                    arg_model.or(cur_model).unwrap_or_default(),
                )
            };

            if base.trim().is_empty() || key.trim().is_empty() || model.trim().is_empty() {
                eprintln!("base_url / api_key / model 均不能为空（参数模式示例：ganyu setup --base_url X --api_key Y --model Z）");
                std::process::exit(2);
            }

            ganyu_agent::config::write_model_config(base.trim(), key.trim(), model.trim())?;
            // 写入后立即生效（env 覆盖规则：setup 显式写入，强制刷新）。
            // F-10：密钥不再写入全局环境，配置文件已落盘，下次启动自动加载。
            let path = ganyu_agent::config::config_path().unwrap_or_default();
            println!("✅ 已写入 {path}");
            println!("   模型: {} ({}，key={})", model.trim(), base.trim(), mask(&Some(key.trim().to_string())).unwrap_or_default());
            println!("   直接对话: ganyu-agent chat（或 ganyu）");
        }
        "update" => {
            // Hermes 式自更新：从 GitHub Releases 下载最新预编译二进制，覆盖 ~/.ganyu/bin。
            #[cfg(feature = "network")]
            {
                let req_version = positional
                    .iter()
                    .find(|a| a.starts_with("v") && a[1..].chars().all(|c| c.is_ascii_digit() || c == '.'))
                    .cloned()
                    .unwrap_or_else(|| "latest".to_string());
                let api_url = format!("https://api.github.com/repos/zuitaiji/ganyu-agent/releases/{req_version}");
                let client = reqwest::Client::builder()
                    .user_agent("ganyu-update")
                    .build()
                    .map_err(|e| GanyuError::Http(e.to_string()))?;
                let release: serde_json::Value = client
                    .get(&api_url)
                    .send().await
                    .map_err(|e| GanyuError::Http(e.to_string()))?
                    .error_for_status()
                    .map_err(|e| GanyuError::Http(e.to_string()))?
                    .json().await
                    .map_err(|e| GanyuError::Http(e.to_string()))?;
                let tag = release["tag_name"].as_str().unwrap_or(&req_version);

                // 平台资产名（与 install.ps1 / release.yml 一致）
                let os = std::env::consts::OS;
                let asset = match os {
                    "windows" => {
                        if std::env::var("PROCESSOR_ARCHITECTURE").as_deref() == Ok("ARM64") {
                            "ganyu-agent-windows-arm64.tar.gz".to_string()
                        } else {
                            "ganyu-agent-windows-x86_64.tar.gz".to_string()
                        }
                    }
                    "linux" => format!(
                        "ganyu-agent-linux-{}.tar.gz",
                        if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" }
                    ),
                    "macos" => format!(
                        "ganyu-agent-macos-{}.tar.gz",
                        if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" }
                    ),
                    other => {
                        eprintln!("暂不支持平台: {other}（可源码编译: git clone + cargo install --features hardened）");
                        std::process::exit(1);
                    }
                };

                let url = release["assets"]
                    .as_array()
                    .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(asset.as_str())))
                    .and_then(|x| x["browser_download_url"].as_str())
                    .map(|u| u.to_string());
                let Some(url) = url else {
                    eprintln!("release {tag} 中未找到资产 {asset}（可能尚未发布，先跑 git tag v0.1.0 && git push --tags）");
                    std::process::exit(1);
                };

                let home = std::env::var("USERPROFILE")
                    .or_else(|_| std::env::var("HOME"))
                    .unwrap_or_else(|_| ".".into());
                let bin_dir = format!("{home}/.ganyu/bin");
                std::fs::create_dir_all(&bin_dir)?;
                let bin_path = format!("{bin_dir}/ganyu-agent{}", if os == "windows" { ".exe" } else { "" });

                println!("[update] {tag} → {bin_path}");
                // 临时文件加随机后缀，避免并发 update 冲突
                let tmp = std::env::temp_dir()
                    .join(format!("{asset}.{}.tmp", uuid::Uuid::new_v4()));
                let bytes = client.get(&url).send().await
                    .map_err(|e| GanyuError::Http(e.to_string()))?
                    .error_for_status()
                    .map_err(|e| GanyuError::Http(e.to_string()))?
                    .bytes().await
                    .map_err(|e| GanyuError::Http(e.to_string()))?;
                std::fs::write(&tmp, &bytes)?;
                println!("[update] 下载完成（{} bytes），校验 sha256…", bytes.len());

                // 供应链校验：下载配套 .sha256 并对比（release 资产由 CI 生成）
                let sha_url = format!("{url}.sha256");
                let sha_bytes = client.get(&sha_url).send().await
                    .map_err(|e| GanyuError::Http(e.to_string()))?
                    .error_for_status()
                    .map_err(|e| GanyuError::Http(e.to_string()))?
                    .bytes().await
                    .map_err(|e| GanyuError::Http(e.to_string()))?;
                let expected = String::from_utf8_lossy(&sha_bytes)
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_lowercase();
                if expected.is_empty() {
                    if std::env::var("GANYU_UPDATE_ALLOW_NOCHECK").is_ok() {
                        eprintln!("[warn] 未获取到 sha256 校验文件，但 GANYU_UPDATE_ALLOW_NOCHECK=1 已设置，跳过校验继续。");
                    } else {
                        eprintln!("[fatal] 未获取到 sha256 校验文件，出于安全考虑拒绝自动应用更新。");
                        eprintln!("        如需继续，请用 GANYU_UPDATE_ALLOW_NOCHECK=1 显式强制覆盖，或手动下载并用 ganyu doctor 校验。");
                        let _ = std::fs::remove_file(&tmp);
                        std::process::exit(1);
                    }
                } else {
                    let actual = sha256_of_file(&tmp)?;
                    if actual != expected {
                        eprintln!("⚠️ sha256 校验失败：资产可能被篡改！");
                        eprintln!("   期望 {expected}");
                        eprintln!("   实际 {actual}");
                        let _ = std::fs::remove_file(&tmp);
                        std::process::exit(1);
                    }
                    println!("[update] sha256 校验通过 ✅");
                }

                // 解压：统一 tar.gz（Windows 10 1803+ 自带 bsdtar；Linux/macOS 自带 tar）。
                use std::process::Command;
                {
                    let list_out = Command::new("tar").args(["-tzf", tmp.to_str().unwrap()]).output();
                    if let Ok(o) = list_out {
                        let entries = String::from_utf8_lossy(&o.stdout);
                        for e in entries.lines() {
                            if e.starts_with('/') || e.starts_with('\\') || e.contains("..") {
                                eprintln!("[fatal] 更新包含非法路径（{e}），疑似路径穿越，已拒绝。");
                                let _ = std::fs::remove_file(&tmp);
                                std::process::exit(1);
                            }
                        }
                    }
                }
                let status = Command::new("tar")
                    .args(["-xzf", tmp.to_str().unwrap(), "-C", &bin_dir])
                    .status()
                    .map_err(|e| GanyuError::Io(e))?;
                if !status.success() {
                    eprintln!("解压失败。请手动解压 {tmp:?} 到 {bin_dir}");
                    std::process::exit(1);
                }
                let _ = std::fs::remove_file(&tmp);

                // 资产内文件名统一为 ganyu-agent（无扩展名）；Windows 需对齐为 ganyu-agent.exe。
                // 运行中的 exe 无法被覆盖（Windows 文件锁）——rename 失败时明确提示。
                if os == "windows" {
                    let unpacked = format!("{bin_dir}/ganyu-agent");
                    if std::path::Path::new(&unpacked).exists() {
                        let _ = std::fs::remove_file(&bin_path);
                        if let Err(e) = std::fs::rename(&unpacked, &bin_path) {
                            eprintln!("⚠️ 替换 {bin_path} 失败: {e}");
                            eprintln!("   新版本已下载到 {unpacked}。请先退出所有 ganyu/ganyu-agent 会话后重试，或手动替换。");
                            std::process::exit(1);
                        }
                    }
                }

                // 别名同步 + 自检
                if os == "windows" {
                    let _ = std::fs::copy(&bin_path, format!("{bin_dir}/ganyu.exe"));
                } else {
                    let _ = std::fs::remove_file(format!("{bin_dir}/ganyu"));
                    let _ = std::fs::hard_link(&bin_path, format!("{bin_dir}/ganyu"));
                }
                println!("[update] ✅ 已更新到 {tag}。运行 ganyu-agent doctor 验证。");
            }
            #[cfg(not(feature = "network"))]
            {
                eprintln!("当前构建无 network 特性，无法联网更新。请用 --features network/hardened 编译，或 git pull + cargo install。");
                std::process::exit(1);
            }
        }
        "model" => {
            // 查看/切换当前模型（本地配置管理；与 `models`（远程列表）区分）。
            let (base, key, cur_model) = ganyu_agent::config::read_model_config();
            if let Some(new_model) = positional.iter().find(|a| !a.starts_with('-')) {
                let base = base.unwrap_or_default();
                let key = key.unwrap_or_default();
                if base.is_empty() || key.is_empty() {
                    eprintln!("未配置端点。先运行 ganyu setup 配置 base_url/api_key 后再切换模型。");
                    std::process::exit(1);
                }
                ganyu_agent::config::write_model_config(&base, &key, new_model)?;
                let prev = cur_model.unwrap_or_default();
                println!("✅ 当前模型已切换: {prev} → {new_model}");
            } else {
                let masked = key
                    .as_deref()
                    .map(|k| {
                        let chars: Vec<char> = k.chars().collect();
                        if chars.len() > 8 {
                            format!(
                                "{}…{}",
                                chars[..4].iter().collect::<String>(),
                                chars[chars.len() - 4..].iter().collect::<String>()
                            )
                        } else {
                            "****".to_string()
                        }
                    })
                    .unwrap_or_else(|| "<未配置>".to_string());
                println!("== 当前模型配置 ==");
                println!("  base_url: {}", base.as_deref().unwrap_or("<未配置>"));
                println!("  api_key : {masked}");
                println!("  model   : {}", cur_model.as_deref().unwrap_or("<未配置>"));
                println!("  切换: ganyu model <新模型id>   查看网关全部可用模型: ganyu models");
            }
        }
        "gateway" => {
            // Telegram 消息平台网关（Hermes 式）：setup 存 token，start 长轮询收发消息。
            let sub = positional
                .first()
                .map(|s| s.as_str())
                .unwrap_or("start");
            match sub {
                "setup" => {
                    #[cfg(feature = "network")]
                    {
                        let token = positional.get(1).cloned().or_else(|| {
                            use std::io::{IsTerminal, Write as _};
                            if std::io::stdin().is_terminal() {
                                print!("Telegram bot token（掩码输入）: ");
                                std::io::stdout().flush().ok();
                                let t = rpassword::read_password().ok()?;
                                let t = t.trim().to_string();
                                if t.is_empty() { None } else { Some(t) }
                            } else { None }
                        });
                        let Some(token) = token else {
                            eprintln!("用法: ganyu gateway setup <bot_token> 或 ganyu gateway setup（交互输入）");
                            std::process::exit(2);
                        };
                        ganyu_agent::config::write_gateway_token(token.trim())?;
                        let path = ganyu_agent::config::config_path().unwrap_or_default();
                        println!("✅ Telegram token 已写入 {path} 的 [gateway] 段");
                        println!("   启动: ganyu gateway start");
                    }
                    #[cfg(not(feature = "network"))]
                    {
                        eprintln!("当前构建无 network 特性，无法接 Telegram。请用 --features network/hardened 编译。");
                        std::process::exit(1);
                    }
                }
                "start" => {
                    #[cfg(feature = "network")]
                    {
                        let token = ganyu_agent::config::read_gateway_token();
                        let Some(token) = token else {
                            eprintln!("未配置 Telegram token。先运行: ganyu gateway setup <bot_token>");
                            std::process::exit(1);
                        };
                        let api = format!("https://api.telegram.org/bot{token}");
                        let client = reqwest::Client::builder()
                            .user_agent("ganyu-gateway")
                            .build()
                            .map_err(|e| GanyuError::Http(e.to_string()))?;
                        // 校验 token
                        let me: serde_json::Value = client
                            .get(format!("{api}/getMe"))
                            .send().await
                            .map_err(|e| GanyuError::Http(e.to_string()))?
                            .error_for_status()
                            .map_err(|e| GanyuError::Http(e.to_string()))?
                            .json().await
                            .map_err(|e| GanyuError::Http(e.to_string()))?;
                        if me["ok"].as_bool() != Some(true) {
                            eprintln!("getMe 失败（token 无效？）: {me}");
                            std::process::exit(1);
                        }
                        let bot_name = me["result"]["username"].as_str().unwrap_or("bot");
                        println!("✅ Telegram 网关已启动（@{bot_name}）。Ctrl+C 退出。");

                        // 会话隔离：每个 chat_id 一个独立 Agent/session（懒创建缓存），
                        // 避免多用户消息串同一上下文。重启网关后为新 session（会话不跨重启）。
                        let mut chat_agents: HashMap<i64, Arc<Agent>> = HashMap::new();
                        let mut offset: i64 = 0;
                        loop {
                            // 长轮询：timeout=25s 保持连接，减少无效请求
                            let updates: serde_json::Value = match client
                                .get(format!("{api}/getUpdates"))
                                .query(&[
                                    ("offset", offset.to_string()),
                                    ("timeout", "25".to_string()),
                                    ("allowed_updates", r#"["message"]"#.to_string()),
                                ])
                                .send().await
                                .map_err(|e| GanyuError::Http(e.to_string()))?
                                .error_for_status()
                                .map_err(|e| GanyuError::Http(e.to_string()))?
                                .json().await
                            {
                                Ok(v) => v,
                                Err(e) => {
                                    eprintln!("[gateway] getUpdates 错误: {e}，3 秒后重试");
                                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                                    continue;
                                }
                            };
                            if updates["ok"].as_bool() != Some(true) {
                                eprintln!("[gateway] getUpdates 返回错误: {updates}");
                                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                                continue;
                            }
                            let Some(arr) = updates["result"].as_array() else { continue };
                            for upd in arr {
                                if let Some(n) = upd["update_id"].as_i64() {
                                    offset = n + 1;
                                }
                                let Some(text) = upd["message"]["text"].as_str() else { continue };
                                let chat_id = upd["message"]["chat"]["id"].as_i64();
                                let Some(chat_id) = chat_id else { continue };
                                let from = upd["message"]["from"]["username"].as_str().unwrap_or("user");
                                let text = text.trim().to_string();
                                if text.is_empty() { continue; }
                                println!("[gateway] @{from}: {text}");
                                // 按 chat_id 隔离会话；首次创建独立 Agent（共享工具/记忆/网关）
                                let chat_agent = chat_agents.entry(chat_id).or_insert_with(|| {
                                    let sid = SessionId::new();
                                    println!("[gateway] 新会话 chat={chat_id} session={sid}");
                                    Arc::new(Agent::new(
                                        agent.gateway.clone(),
                                        memory.clone(),
                                        tools.clone(),
                                        skills.clone(),
                                        agent.reasoner.clone(),
                                        sid,
                                    ))
                                });
                                // 用该会话的 agent 推理
                                let out = match chat_agent.run(&Value(text.clone())).await {
                                    Ok(v) => v.to_string(),
                                    Err(e) => format!("抱歉，处理出错: {e}"),
                                };
                                println!("[gateway] ganyu: {out}");
                                // 回复（截断过长消息，Telegram 限制 4096）
                                let reply: String = out.chars().take(4000).collect();
                                let _ = client
                                    .post(format!("{api}/sendMessage"))
                                    .json(&serde_json::json!({
                                        "chat_id": chat_id,
                                        "text": reply,
                                    }))
                                    .send().await;
                            }
                        }
                    }
                    #[cfg(not(feature = "network"))]
                    {
                        eprintln!("当前构建无 network 特性，无法接 Telegram。请用 --features network/hardened 编译。");
                        std::process::exit(1);
                    }
                }
                other => {
                    eprintln!("用法: ganyu gateway setup <bot_token> | ganyu gateway start");
                    eprintln!("未知子命令: {other}");
                    std::process::exit(2);
                }
            }
        }
        "agent" => {
            let mode = mode_arg.clone().unwrap_or_else(|| "react".to_string());
            let query = positional.join(" ");
            if query.trim().is_empty() {
                eprintln!("用法: cargo run -- agent \"任务\" --mode <single|react|plan|multi|router|blackboard|graph>");
                std::process::exit(2);
            }
            let ctx = RunContext::new(
                session,
                memory.clone(),
                agent.gateway.clone(),
                tools.clone(),
                skills.clone(),
            );
            let wf = build_workflow(&mode, &ctx, &agent)?;
            println!("session: {session}");
            println!("mode: {}", wf.mode());
            let out = wf.run(&ctx, &Value(query)).await?;
            println!("\n>> {}", out);
        }
        _ => {
            use std::io::{IsTerminal, Read, Write};
            println!("session: {session}");
            let resumed = agent.resume().await;
            if resumed {
                println!("[已续接会话 {session}]");
            }
            if std::io::stdin().is_terminal() {
                // 交互式 REPL（对标 OpenClaw / Hermes 的对话体验）：
                // 多轮对话共享同一会话，记忆/上下文跨轮延续；输入 /quit 或 Ctrl+C 退出。
                println!("ganyu-agent 交互对话已启动（同一会话延续上下文；输入 /quit 或 Ctrl+C 退出）");
                // F-10：key 不注入全局 env，从 env 或 config.toml 取
                let key_ok = std::env::var("OPENAI_API_KEY").is_ok()
                    || ganyu_agent::config::read_model_config().1.is_some();
                if std::env::var("OPENAI_API_BASE").is_err() || !key_ok {
                    println!("⚠️ 未配置模型（当前为离线本地兜底）。编辑 ~/.ganyu/config.toml 的 [model] 段，或运行 ganyu-agent doctor 查看指引。");
                } else {
                    println!("💡 已连接模型：{}", std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "默认".into()));
                }
                let mut line = String::new();
                loop {
                    print!("ganyu> ");
                    std::io::stdout().flush().ok();
                    line.clear();
                    let n = std::io::stdin().read_line(&mut line).unwrap_or(0);
                    if n == 0 {
                        // EOF（Ctrl+Z / Ctrl+C）退出
                        println!();
                        break;
                    }
                    let msg = line.trim().to_string();
                    if msg.is_empty() {
                        continue;
                    }
                    if msg == "/quit" || msg == "/exit" || msg == "/q" {
                        break;
                    }
                    match agent.run(&Value(msg)).await {
                        Ok(out) => println!("\n>> {out}\n"),
                        Err(e) => eprintln!("error: {e}"),
                    }
                }
                println!("再见。");
            } else {
                // 管道/单次：读全部内容跑一次（向后兼容：echo "...| ganyu-agent chat"）。
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf).ok();
                let msg = buf.trim().to_string();
                let out = if msg.is_empty() {
                    agent.run(&Value("你好".into())).await?
                } else {
                    agent.run(&Value(msg)).await?
                };
                println!("{out}");
            }
        }
    }
    Ok(())
}

fn print_trace(agent: &ganyu_agent::core::Agent) {
    println!("\n== 推理轨迹 ==");
    for (i, step) in agent.trace().iter().enumerate() {
        match step {
            Step::Thought(s) => println!("  {}. 💭 {s}", i + 1),
            Step::Action { tool, args } => println!("  {}. ⚡ @{tool} {args}", i + 1),
            Step::Observation(s) => println!("  {}. 👁 {s}", i + 1),
            Step::Final(s) => println!("  {}. ✅ {s}", i + 1),
        }
    }
}

/// 按范式名构造对应 `Workflow`。所有单元复用同一份网关/记忆/工具/技能/会话（Arc 克隆）。
fn build_workflow(
    mode: &str,
    _ctx: &RunContext,
    base: &Agent,
) -> GanyuResult<Arc<dyn Workflow>> {
    // 角色化构造一个 Unit（Agent），共享 base 的全部后端。
    let mk = |role: &str| -> Arc<dyn Unit> {
        Arc::new(Agent::with_role(
            base.gateway.clone(),
            base.memory.clone(),
            base.tools.clone(),
            base.skills.clone(),
            Arc::new(LocalReasoner),
            base.session,
            role,
        ))
    };
    let wf: Arc<dyn Workflow> = match mode {
        "single" | "react" => Arc::new(SingleWorkflow::new(mk(""))),
        "plan" => Arc::new(PlanExecuteWorkflow::new(Arc::new(LocalPlanner), mk(""))),
        "multi" => Arc::new(MultiAgentWorkflow::new(
            vec![mk("规划者"), mk("执行者"), mk("复核者")],
            2,
        )),
        "router" => {
            let mut routes: HashMap<String, Arc<dyn Unit>> = HashMap::new();
            routes.insert("summarize".into(), mk("Summarizer"));
            routes.insert("troubleshoot".into(), mk("Troubleshooter"));
            routes.insert("kb".into(), mk("KB"));
            let router = Arc::new(KeywordRouter::new(vec![
                ("总结", "summarize"),
                ("摘要", "summarize"),
                ("summarize", "summarize"),
                ("排查", "troubleshoot"),
                ("故障", "troubleshoot"),
                ("报错", "troubleshoot"),
                ("troubleshoot", "troubleshoot"),
                ("知识库", "kb"),
                ("kb", "kb"),
                ("查一下", "kb"),
            ]));
            Arc::new(RouterWorkflow::new(router, routes, mk("")))
        }
        "blackboard" => Arc::new(BlackboardWorkflow::new(
            vec![mk("研究员"), mk("写作者")],
            mk("合成者"),
            1,
        )),
        "graph" => {
            let w = GraphBuilder::default()
                .node("research", mk("研究员"))
                .edge("research", "draft")
                .node("draft", mk("写作者"))
                .edge("draft", "review")
                .node("review", mk("复核者"))
                .end("review")
                .build()?;
            Arc::new(w)
        }
        other => {
            return Err(GanyuError::Workflow(format!(
                "未知范式：{other}（用 `modes` 查看支持列表）"
            )))
        }
    };
    Ok(wf)
}

async fn selftest() {
    // 自检使用 CWD 作为文件沙箱根（测试环境），保持与历史行为/清理一致；
    // 真正运行时默认是隔离的 `.ganyu_workspace`（C3/C4）。
    std::env::set_var("GANYU_FS_ROOT", ".");
    let mut pass = 0usize;
    let mut fail = 0usize;
    macro_rules! check {
        ($name:expr, $cond:expr) => {
            if $cond {
                pass += 1;
                println!("PASS  {}", $name);
            } else {
                fail += 1;
                println!("FAIL  {}", $name);
            }
        };
    }

    // 1) 重试自愈
    let mut n = 0;
    let r = with_retry(
        || {
            n += 1;
            if n < 3 {
                Err("x")
            } else {
                Ok("ok")
            }
        },
        5,
        std::time::Duration::from_millis(1),
    );
    check!("with_retry 重试后成功", r == Ok("ok"));

    // 2) SAG 端到端（本地，无网络）——依赖 examples/sample_mdl.json；
    //    release 免编译安装没有 examples/，文件缺失时降级为跳过（不 panic）。
    let memory: DynMemory = Arc::new(LocalMemory::new(".ganyu_selftest_mem.json"));
    let gw = Gateway::new();
    gw.register(Arc::new(LocalBackend) as DynBackend);
    let skills = Arc::new(SkillBook::new(memory.clone()));
    register_core_skills(&skills);
    if let Ok(mdl) = Mdl::load("examples/sample_mdl.json") {
        let pipe = SagPipeline {
            mdl: Arc::new(mdl),
            gateway: Arc::new(gw),
            memory: memory.clone(),
            skills,
            session: SessionId::new(),
        };
        let out = pipe
            .run(&Value("上月华东区利润最高的三个产品".into()), None)
            .await
            .unwrap();
        check!("SAG 生成并通过 MDL 校验", out.verdict == Verdict::Pass);
        check!(
            "SAG 产出合法 SQL",
            out.sql.as_str().contains("SELECT") && out.sql.as_str().contains("profit")
        );
    } else {
        check!("SAG 端到端（需 examples/sample_mdl.json，release 分发跳过）", true);
    }

    // 2b) 会话记忆持久化 + 续接
    let sid = SessionId::new();
    let mem2: DynMemory = Arc::new(LocalMemory::new(".ganyu_selftest_sess.json"));
    mem2.commit(&sid, &Value("user: 上月华东利润Top3".into()))
        .await
        .unwrap();
    let loaded = mem2.load_session(&sid).await.unwrap();
    check!(
        "会话轨迹可持久化并续接",
        loaded == Some(Value("user: 上月华东利润Top3".into()))
    );
    let _ = std::fs::remove_file(".ganyu_selftest_sess.json");

    // 3) 内置工具（含文件/记忆）
    let mem3: DynMemory = Arc::new(LocalMemory::new(".ganyu_selftest_tools.json"));
    let reg = Arc::new(ToolRegistry::new());
    register_core_tools(&reg, mem3.clone());
    let c = reg.call("calc", &Value("(1+2)*3".into())).await.unwrap();
    check!("内置 calc 工具", c == Value("9".to_string()));
    let p = ".ganyu_selftest_file.txt";
    let _ = std::fs::remove_file(p);
    reg.call("file_write", &Value(format!("{p}\nhello")))
        .await
        .unwrap();
    let rd = reg.call("file_read", &Value(p.into())).await.unwrap();
    check!("file_write/file_read 工具", rd == Value("hello".into()));
    reg.call("remember", &Value("k\nv".into())).await.unwrap();
    let rc = reg.call("recall", &Value("k".into())).await.unwrap();
    check!("remember/recall 工具", rc == Value("v".into()));
    let _ = std::fs::remove_file(p);
    let _ = std::fs::remove_file(".ganyu_selftest_file.txt");
    let _ = std::fs::remove_file(".ganyu_selftest_tools.json");

    // 3b) 特性技能 summarize
    let book = Arc::new(SkillBook::new(mem3.clone()));
    register_core_skills(&book);
    for name in book.skill_names() {
        let desc = book.get_skill(&name).map(|s| s.description.clone()).unwrap_or_default();
        reg.register(Arc::new(SkillTool::new(
            book.clone(),
            reg.clone(),
            name.clone(),
            desc,
        )));
    }
    let sp = ".ganyu_selftest_skill.txt";
    let _ = std::fs::remove_file(sp);
    reg.call("file_write", &Value(format!("{sp}\nalpha\nbeta\ngamma")))
        .await
        .unwrap();
    let sum = reg.call("skill:summarize", &Value(sp.into())).await.unwrap();
    check!("特性技能 summarize", sum.as_str().contains("摘要") && sum.as_str().contains("3 行"));
    let _ = std::fs::remove_file(sp);

    // 4) 网关级联 fallback（Fail -> Ok）
    struct FailBackend;
    #[async_trait::async_trait]
    impl LlmBackend for FailBackend {
        fn name(&self) -> &str {
            "fail"
        }
        async fn complete(&self, _: &[Message]) -> GanyuResult<Value> {
            Err(GanyuError::BackendUnavailable("fail".into()))
        }
    }
    struct OkBackend;
    #[async_trait::async_trait]
    impl LlmBackend for OkBackend {
        fn name(&self) -> &str {
            "ok"
        }
        async fn complete(&self, _: &[Message]) -> GanyuResult<Value> {
            Ok(Value("ok".into()))
        }
    }
    let g2 = Gateway::new();
    g2.register(Arc::new(FailBackend) as DynBackend);
    g2.register(Arc::new(OkBackend) as DynBackend);
    let r = g2.complete(&[Message::user("hi")]).await;
    check!("网关级联 fallback", matches!(r, Ok(v) if v.as_str() == "ok"));

    println!("\nselftest: {pass} passed, {fail} failed");
    let _ = std::fs::remove_file(".ganyu_selftest_mem.json");
    if fail > 0 {
        std::process::exit(1);
    }
}
