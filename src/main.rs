//! ganyu-agent CLI。
//!
//! 子命令：
//! - `sag "问题"`  ：跑知识/分析面的 SAG 管道（默认 examples/sample_mdl.json）。
//! - `selftest`    ：运行内置自愈/可拓展自检（无需 cargo test）。
//! - `chat`        ：读 stdin 一行，经 Agent 对话面响应。

use std::path::Path;
use std::sync::Arc;

use ganyu_agent::core::llm::{DynBackend, LlmBackend, LocalBackend, Message};
#[cfg(feature = "network")]
use ganyu_agent::core::llm::OpenAiBackend;
use ganyu_agent::core::memory::{DynMemory, LocalMemory};
use ganyu_agent::ext::{register_builtins, SkillBook, ToolRegistry};
use ganyu_agent::heal::with_retry;
use ganyu_agent::knowledge::mdl::Mdl;
use ganyu_agent::knowledge::sag::{SagPipeline, Verdict};
use ganyu_agent::routing::Gateway;
use ganyu_agent::session::SessionId;
use ganyu_agent::value::Value;
use ganyu_agent::{GanyuError, GanyuResult};

#[tokio::main]
async fn main() -> GanyuResult<()> {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("chat");

    let session = SessionId::new();
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

    let tools = Arc::new(ToolRegistry::new());
    register_builtins(&tools);
    let _ = tools.discover(Path::new("plugins"));

    let skills = Arc::new(SkillBook::new(memory.clone()));
    let agent = ganyu_agent::core::Agent::new(
        Arc::new(gateway),
        memory.clone(),
        tools.clone(),
        skills,
        session,
    );

    match cmd {
        "sag" => {
            let query = args
                .get(2)
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
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf).ok();
            let msg = buf.trim().to_string();
            if msg.is_empty() {
                println!("{}", agent.respond(&Value("你好".into())).await?);
            } else {
                println!("{}", agent.respond(&Value(msg)).await?);
            }
        }
    }
    Ok(())
}

async fn selftest() {
    use ganyu_agent::core::memory::{DynMemory, LocalMemory};
    use ganyu_agent::ext::{register_builtins, SkillBook, ToolRegistry};

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
    check!(
        "SAG 生成并通过 MDL 校验",
        out.verdict == Verdict::Pass
    );
    check!(
        "SAG 产出合法 SQL",
        out.sql.as_str().contains("SELECT") && out.sql.as_str().contains("profit")
    );

    // 3) 工具注册与调用
    let tools = ToolRegistry::new();
    register_builtins(&tools);
    let r = tools.call("calc", &Value("(1+2)*3".into())).await.unwrap();
    check!("内置 calc 工具", r == Value("9".to_string()));

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
