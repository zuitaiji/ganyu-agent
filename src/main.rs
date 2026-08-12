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
        if cmd.is_none() {
            cmd = Some(raw[k].clone());
            k += 1;
            continue;
        }
        positional.push(raw[k].clone());
        k += 1;
    }
    let cmd = cmd.as_deref().unwrap_or("chat");

    let memory: DynMemory = Arc::new(LocalMemory::new(".ganyu_memory.json"));

    // 工程化配置面：集中读取 GANYU_*，据此启用缓存/限速/审计。
    let cfg = ganyu_agent::config::GanyuConfig::from_env();
    // 一站式：从 ~/.ganyu/config.toml 加载模型配置（已设置的环境变量优先）。
    ganyu_agent::config::load_model_config();
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

    if let (Ok(base), Ok(key)) = (
        std::env::var("OPENAI_API_BASE"),
        std::env::var("OPENAI_API_KEY"),
    ) {
        // 模型名可用 OPENAI_MODEL 覆盖（默认 gpt-4o-mini；OpenAI 兼容端点均可）。
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
        #[cfg(feature = "network")]
        gateway.register(Arc::new(OpenAiBackend::new(&base, &key, &model)) as DynBackend);
        #[cfg(not(feature = "network"))]
        let _ = (base, key);
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
        let has_model = std::env::var("OPENAI_API_BASE").is_ok()
            && std::env::var("OPENAI_API_KEY").is_ok();
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
            let key_set = std::env::var("OPENAI_API_KEY").is_ok();
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
            let mem_ok = std::path::Path::new(".ganyu_memory.json").exists();
            println!(
                "记忆文件 : .ganyu_memory.json [{}]",
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

    // 2) SAG 端到端（本地，无网络）
    let memory: DynMemory = Arc::new(LocalMemory::new(".ganyu_selftest_mem.json"));
    let gw = Gateway::new();
    gw.register(Arc::new(LocalBackend) as DynBackend);
    let skills = Arc::new(SkillBook::new(memory.clone()));
    register_core_skills(&skills);
    let mdl = Mdl::load("examples/sample_mdl.json").unwrap();
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
