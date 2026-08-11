//! 集成测试：通过 lib 端到端验证 SAG、内置工具、技能与 ReAct 推理循环。

use ganyu_agent::core::llm::LocalBackend;
use ganyu_agent::core::loop_::LocalReasoner;
use ganyu_agent::core::memory::{DynMemory, LocalMemory};
use ganyu_agent::ext::builtins::register_core_tools;
use ganyu_agent::ext::skills::{register_core_skills, SkillTool};
use ganyu_agent::ext::{SkillBook, ToolRegistry};
use ganyu_agent::knowledge::mdl::Mdl;
use ganyu_agent::knowledge::sag::{SagPipeline, Verdict};
use ganyu_agent::routing::Gateway;
use ganyu_agent::session::SessionId;
use ganyu_agent::value::Value;
use std::sync::Arc;

fn build_agent() -> (ganyu_agent::core::Agent, std::path::PathBuf) {
    let mem_path = ".ganyu_it_mem.json";
    let memory: DynMemory = Arc::new(LocalMemory::new(mem_path));
    let mut gw = Gateway::new();
    gw.register(Arc::new(LocalBackend));
    let tools = Arc::new(ToolRegistry::new());
    register_core_tools(&tools, memory.clone());
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
        Arc::new(gw),
        memory.clone(),
        tools.clone(),
        skills.clone(),
        reasoner,
        SessionId::new(),
    );
    (agent, std::path::PathBuf::from(mem_path))
}

#[tokio::test]
async fn sag_end_to_end_local() {
    let memory: DynMemory = Arc::new(LocalMemory::new(".ganyu_it_sag.json"));
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
    let _ = std::fs::remove_file(".ganyu_it_sag.json");
}

#[tokio::test]
async fn tool_echo_and_calc() {
    let memory: DynMemory = Arc::new(LocalMemory::new(".ganyu_it_tools.json"));
    let reg = Arc::new(ToolRegistry::new());
    register_core_tools(&reg, memory.clone());
    assert_eq!(
        reg.call("echo", &Value("hi".into())).await.unwrap(),
        Value("hi".into())
    );
    assert_eq!(
        reg.call("calc", &Value("2+3*4".into())).await.unwrap(),
        Value("14".to_string())
    );
    let _ = std::fs::remove_file(".ganyu_it_tools.json");
}

#[tokio::test]
async fn agent_run_single_step_final() {
    let (agent, mem) = build_agent();
    let out = agent.run(&Value("@calc 2+3".into())).await.unwrap();
    assert_eq!(out, Value("5".to_string()));
    let _ = std::fs::remove_file(mem);
}

#[tokio::test]
async fn agent_run_multistep_trace() {
    let (agent, mem) = build_agent();
    // 多步脚本：写文件 + 算术；轨迹应含 2 个 Action。
    let out = agent
        .run(&Value("@file_write .ganyu_it_run.txt\nhello\n@calc 2+2".into()))
        .await
        .unwrap();
    let steps = agent.trace();
    let actions = steps
        .iter()
        .filter(|s| matches!(s, ganyu_agent::core::Step::Action { .. }))
        .count();
    assert!(actions >= 2, "expected >=2 actions, got {actions}");
    // 末步为 Final（含遗留文本兜底）。
    assert!(matches!(steps.last(), Some(ganyu_agent::core::Step::Final(_))));
    assert!(!out.as_str().is_empty());
    let _ = std::fs::remove_file(mem);
    let _ = std::fs::remove_file(".ganyu_it_run.txt");
}

#[tokio::test]
async fn agent_runs_skill_via_reasoner() {
    let (agent, mem) = build_agent();
    // 直接写文件（多行内容走 tools.call；@脚本语法仅传同行参数）。
    agent
        .tools
        .call(
            "file_write",
            &Value(".ganyu_it_skill.txt\nalpha\nbeta\ngamma".into()),
        )
        .await
        .unwrap();
    // 经推理器路由到特性技能 summarize。
    let out = agent
        .run(&Value("@skill:summarize .ganyu_it_skill.txt".into()))
        .await
        .unwrap();
    assert!(out.as_str().contains("摘要"));
    assert!(out.as_str().contains("3 行"));
    let _ = std::fs::remove_file(mem);
    let _ = std::fs::remove_file(".ganyu_it_skill.txt");
}
