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
use ganyu_agent::core::loop_::{LocalReasoner, Step};
use ganyu_agent::core::memory::{DynMemory, LocalMemory};
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

#[tokio::main]
async fn main() -> GanyuResult<()> {
    let raw: Vec<String> = std::env::args().collect();
    let mut session = SessionId::new();
    let mut cmd: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
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
    let mut gateway = Gateway::new();
    gateway.register(Arc::new(LocalBackend) as DynBackend);

    if let (Ok(base), Ok(key)) = (
        std::env::var("OPENAI_API_BASE"),
        std::env::var("OPENAI_API_KEY"),
    ) {
        #[cfg(feature = "network")]
        gateway.register(Arc::new(OpenAiBackend::new(&base, &key, "gpt-4o-mini")) as DynBackend);
        #[cfg(not(feature = "network"))]
        let _ = (base, key);
    }

    // 工具层：内置能力 + 插件发现
    let tools = Arc::new(ToolRegistry::new());
    register_core_tools(&tools, memory.clone());
    let _ = tools.discover(Path::new("plugins"));

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

    let reasoner: Arc<dyn ganyu_agent::core::loop_::Reasoner> = Arc::new(LocalReasoner);
    let agent = ganyu_agent::core::Agent::new(
        Arc::new(gateway),
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
        _ => {
            use std::io::Read;
            println!("session: {session}");
            let resumed = agent.resume().await;
            if resumed {
                println!("[已续接会话 {}]", session);
            }
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf).ok();
            let msg = buf.trim().to_string();
            if msg.is_empty() {
                println!("{}", agent.run(&Value("你好".into())).await?);
            } else {
                println!("{}", agent.run(&Value(msg)).await?);
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

async fn selftest() {
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
    let mut gw = Gateway::new();
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
    let mut g2 = Gateway::new();
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
