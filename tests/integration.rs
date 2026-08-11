//! 集成测试：通过 lib 端到端验证 SAG 与工具层。

use ganyu_agent::core::llm::LocalBackend;
use ganyu_agent::core::memory::{DynMemory, LocalMemory};
use ganyu_agent::ext::{register_builtins, SkillBook, ToolRegistry};
use ganyu_agent::knowledge::mdl::Mdl;
use ganyu_agent::knowledge::sag::{SagPipeline, Verdict};
use ganyu_agent::routing::Gateway;
use ganyu_agent::session::SessionId;
use ganyu_agent::value::Value;
use std::sync::Arc;

#[tokio::test]
async fn sag_end_to_end_local() {
    let memory: DynMemory = Arc::new(LocalMemory::new(".ganyu_it_mem.json"));
    let mut gw = Gateway::new();
    gw.register(Arc::new(LocalBackend));
    let skills = Arc::new(SkillBook::new(memory.clone()));
    let mdl = Mdl::load("examples/sample_mdl.json").unwrap();
    let pipe = SagPipeline {
        mdl: Arc::new(mdl),
        gateway: Arc::new(gw),
        memory,
        skills,
        session: SessionId::new(),
    };
    let out = pipe
        .run(&Value("上月华东区利润最高的三个产品".into()), None)
        .await
        .unwrap();
    assert_eq!(out.verdict, Verdict::Pass);
    assert!(out.sql.as_str().contains("profit"));
    let _ = std::fs::remove_file(".ganyu_it_mem.json");
}

#[tokio::test]
async fn tool_echo_and_calc() {
    let reg = ToolRegistry::new();
    register_builtins(&reg);
    assert_eq!(
        reg.call("echo", &Value("hi".into())).await.unwrap(),
        Value("hi".into())
    );
    assert_eq!(
        reg.call("calc", &Value("2+3*4".into())).await.unwrap(),
        Value("14".to_string())
    );
}
