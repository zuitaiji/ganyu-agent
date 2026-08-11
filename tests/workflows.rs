//! 多范式编排集成测试：覆盖 single/react/plan/multi/router/blackboard/graph。
//! 全部离线（LocalBackend / LocalReasoner 兜底），无需网络或密钥。

use std::sync::Arc;

use async_trait::async_trait;

use ganyu_agent::core::agent::Agent;
use ganyu_agent::core::llm::{DynBackend, LocalBackend};
use ganyu_agent::core::loop_::LocalReasoner;
use ganyu_agent::core::memory::{DynMemory, LocalMemory};
use ganyu_agent::core::unit::{RunContext, Unit};
use ganyu_agent::core::workflow::{
    BlackboardWorkflow, GraphBuilder, KeywordRouter, LocalPlanner, MultiAgentWorkflow,
    PlanExecuteWorkflow, RouterWorkflow, SingleWorkflow, Workflow,
};
use ganyu_agent::ext::builtins::register_core_tools;
use ganyu_agent::ext::skills::{register_core_skills, SkillTool};
use ganyu_agent::ext::{SkillBook, ToolRegistry};
use ganyu_agent::routing::Gateway;
use ganyu_agent::session::SessionId;
use ganyu_agent::value::Value;

/// 固定输出单元：用于断言编排行为（路由/黑板/图）。
struct ConstUnit {
    name: String,
    out: String,
}

#[async_trait]
impl Unit for ConstUnit {
    fn name(&self) -> &str {
        &self.name
    }
    async fn run(&self, _ctx: &RunContext, _input: &Value) -> ganyu_agent::GanyuResult<Value> {
        Ok(Value(self.out.clone()))
    }
}

fn ctx(tag: &str) -> RunContext {
    let mem: DynMemory = Arc::new(LocalMemory::new(format!(".ganyu_wf_{tag}.json")));
    let mut gw = Gateway::new();
    gw.register(Arc::new(LocalBackend) as DynBackend);
    let tools = Arc::new(ToolRegistry::new());
    register_core_tools(&tools, mem.clone());
    let skills = Arc::new(SkillBook::new(mem.clone()));
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
    RunContext::new(SessionId::new(), mem, Arc::new(gw), tools, skills)
}

fn agent_unit(role: &str, tag: &str) -> Arc<dyn Unit> {
    let mem: DynMemory = Arc::new(LocalMemory::new(format!(".ganyu_wf_agent_{tag}.json")));
    let mut gw = Gateway::new();
    gw.register(Arc::new(LocalBackend) as DynBackend);
    let tools = Arc::new(ToolRegistry::new());
    register_core_tools(&tools, mem.clone());
    let skills = Arc::new(SkillBook::new(mem.clone()));
    register_core_skills(&skills);
    let gw = Arc::new(gw);
    Arc::new(Agent::with_role(
        gw,
        mem,
        tools,
        skills,
        Arc::new(LocalReasoner),
        SessionId::new(),
        role,
    ))
}

#[tokio::test]
async fn single_runs_unit_and_writes_board() {
    let c = ctx("single");
    let wf = SingleWorkflow::new(agent_unit("agent", "single"));
    let out = wf.run(&c, &Value("你好".into())).await.unwrap();
    assert!(!out.as_str().is_empty());
    assert!(c.board_get("agent").is_some());
}

#[test]
fn local_planner_decomposes_on_connectors() {
    let c = ctx("plan0");
    let p = LocalPlanner;
    let v = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { p.run(&c, &Value("总结报告然后排查错误".into())).await })
        .unwrap();
    assert!(v.as_str().contains("总结报告"));
    assert!(v.as_str().contains("排查错误"));
}

#[tokio::test]
async fn plan_execute_runs_end_to_end() {
    let c = ctx("plan");
    let wf = PlanExecuteWorkflow::new(Arc::new(LocalPlanner), agent_unit("exec", "plan"));
    let out = wf
        .run(&c, &Value("总结需求文档然后排查潜在故障".into()))
        .await
        .unwrap();
    assert!(!out.as_str().is_empty());
}

#[tokio::test]
async fn router_dispatches_by_keyword_and_falls_back() {
    let c = ctx("router");
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        "summarize".to_string(),
        Arc::new(ConstUnit {
            name: "Summarizer".into(),
            out: "SUMMARIZED".into(),
        }) as Arc<dyn Unit>,
    );
    let router = Arc::new(KeywordRouter::new(vec![("总结", "summarize"), ("summarize", "summarize")]));
    let wf = RouterWorkflow::new(router, routes, Arc::new(ConstUnit {
        name: "fallback".into(),
        out: "FALLBACK".into(),
    }));

    let routed = wf
        .run(&c, &Value("帮我总结一下这段内容".into()))
        .await
        .unwrap();
    assert_eq!(routed.as_str(), "SUMMARIZED");

    let fell = wf.run(&c, &Value("随便聊聊".into())).await.unwrap();
    assert_eq!(fell.as_str(), "FALLBACK");
}

#[tokio::test]
async fn multi_agent_produces_collaborative_transcript() {
    let c = ctx("multi");
    let wf = MultiAgentWorkflow::new(
        vec![
            Arc::new(ConstUnit {
                name: "规划者".into(),
                out: "计划A".into(),
            }),
            Arc::new(ConstUnit {
                name: "执行者".into(),
                out: "执行A".into(),
            }),
        ],
        1,
    );
    let out = wf.run(&c, &Value("做一件事".into())).await.unwrap();
    assert!(out.as_str().contains("规划者"));
    assert!(out.as_str().contains("执行者"));
    assert!(out.as_str().contains("计划A"));
}

#[tokio::test]
async fn blackboard_populates_shared_state() {
    let c = ctx("bb");
    let wf = BlackboardWorkflow::new(
        vec![agent_unit("研究员", "bb1"), agent_unit("写作者", "bb2")],
        Arc::new(ConstUnit {
            name: "合成者".into(),
            out: "合成完成".into(),
        }),
        1,
    );
    let out = wf.run(&c, &Value("写一份季度报告".into())).await.unwrap();
    assert_eq!(out.as_str(), "合成完成");
    // 黑板已被各 agent 贡献填充
    let board = c.board_all();
    assert!(board.contains_key("problem"));
    assert!(board.contains_key("研究员"));
    assert!(board.contains_key("写作者"));
}

#[tokio::test]
async fn graph_executes_dag_in_order() {
    let c = ctx("graph");
    let wf = GraphBuilder::default()
        .node(
            "research",
            Arc::new(ConstUnit {
                name: "r".into(),
                out: "R1".into(),
            }),
        )
        .edge("research", "draft")
        .node(
            "draft",
            Arc::new(ConstUnit {
                name: "d".into(),
                out: "D1".into(),
            }),
        )
        .edge("draft", "review")
        .node(
            "review",
            Arc::new(ConstUnit {
                name: "v".into(),
                out: "V1".into(),
            }),
        )
        .end("review")
        .build()
        .unwrap();
    let out = wf.run(&c, &Value("主题X".into())).await.unwrap();
    assert_eq!(out.as_str(), "V1");
}

#[tokio::test]
async fn graph_detects_cycle() {
    let c = ctx("cycle");
    let r = GraphBuilder::default()
        .node("a", Arc::new(ConstUnit { name: "a".into(), out: "x".into() }))
        .node("b", Arc::new(ConstUnit { name: "b".into(), out: "y".into() }))
        .edge("a", "b")
        .edge("b", "a")
        .end("b")
        .build();
    assert!(r.is_err());
}
