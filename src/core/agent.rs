//! 对话/执行面编排：`Agent`。
//!
//! 把人格层（persona）、推理循环（loop）、路由层（gateway）、记忆层（memory）、
//! 工具层（tools）、技能层（skills）串成「感知—推理—行动—观察」的多步工作流。
//! 任何失败都走 `heal` 自愈（工具重试、网关熔断级联），最终降级到本地兜底而非崩溃。

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::core::loop_::{Decision, Reasoner, Step};
use crate::core::memory::DynMemory;
use crate::core::unit::{RunContext, Unit};
use crate::error::GanyuResult;
use crate::ext::{SkillBook, ToolRegistry};
use crate::persona::build_system_prompt;
use crate::session::SessionId;
use crate::value::Value;

pub struct Agent {
    pub gateway: Arc<crate::routing::Gateway>,
    pub memory: DynMemory,
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillBook>,
    pub reasoner: Arc<dyn Reasoner>,
    pub persona: Value,
    pub role: String,
    pub session: SessionId,
    steps: Mutex<Vec<Step>>,
}

impl Agent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        gateway: Arc<crate::routing::Gateway>,
        memory: DynMemory,
        tools: Arc<ToolRegistry>,
        skills: Arc<SkillBook>,
        reasoner: Arc<dyn Reasoner>,
        session: SessionId,
    ) -> Self {
        Self::with_role(gateway, memory, tools, skills, reasoner, session, "")
    }

    /// 带角色构造：角色会注入人格 system prompt，用于多 agent / Router / Blackboard 区分职责。
    #[allow(clippy::too_many_arguments)]
    pub fn with_role(
        gateway: Arc<crate::routing::Gateway>,
        memory: DynMemory,
        tools: Arc<ToolRegistry>,
        skills: Arc<SkillBook>,
        reasoner: Arc<dyn Reasoner>,
        session: SessionId,
        role: &str,
    ) -> Self {
        let persona = if role.is_empty() {
            build_system_prompt("")
        } else {
            build_system_prompt(&format!(
                "你的专属角色：{role}。请始终以该角色的专业视角拆解与作答。"
            ))
        };
        Agent {
            gateway,
            memory,
            tools,
            skills,
            reasoner,
            persona,
            role: role.to_string(),
            session,
            steps: Mutex::new(Vec::new()),
        }
    }

    pub fn session(&self) -> SessionId {
        self.session
    }

    /// 运行一次完整推理循环（ReAct）。返回最终作答，过程写入可观测轨迹。
    pub async fn run(&self, user_msg: &Value) -> GanyuResult<Value> {
        self.steps.lock().unwrap().clear();

        // 人格作为首位 Thought，便于续接与可观测。
        self.push(Step::Thought(format!(
            "人格已加载（Pi-EQ，{} 字符）",
            self.persona.as_str().len()
        )));

        // 自然语言意图自动路由到特性技能（无 @ 前缀时）。
        let mut msg = user_msg.as_str().to_string();
        if !msg.trim_start().starts_with('@') {
            if let Some(skill) = self.skills.match_intent(&msg) {
                msg = format!("@skill:{skill} {msg}");
            }
        }

        let known: HashSet<String> = self.tools.names().into_iter().collect();
        const MAX_STEPS: usize = 8;
        let mut final_answer = Value::default();

        for _ in 0..MAX_STEPS {
            let decision = self.reasoner.decide(&msg, &known).await?;
            match decision {
                Decision::Final(text) => {
                    self.push(Step::Final(text.clone()));
                    final_answer = Value(text);
                    break;
                }
                Decision::Act { tool, args, remaining } => {
                    self.push(Step::Action {
                        tool: tool.clone(),
                        args: args.clone(),
                    });
                    // 工具失败也作为 Observation 回流（自愈：让后续步骤据此调整）。
                    let obs = match self.tools.call(&tool, &Value(args.clone())).await {
                        Ok(v) => v,
                        Err(e) => Value(format!("[工具 {tool} 执行失败：{e}]")),
                    };
                    self.push(Step::Observation(obs.as_str().to_string()));
                    if remaining.trim().is_empty() {
                        self.push(Step::Final(obs.as_str().to_string()));
                        final_answer = obs;
                        break;
                    }
                    msg = remaining;
                }
            }
        }

        // 记忆层自愈：会话轨迹写本地（失败不致命）。
        let trace = Value(
            serde_json::json!({
                "session": self.session.as_string(),
                "steps": *self.steps.lock().unwrap(),
                "final": final_answer.as_str(),
            })
            .to_string(),
        );
        let _ = self.memory.commit(&self.session, &trace).await;

        Ok(final_answer)
    }

    /// 对话面入口：等价于一次完整推理循环。
    pub async fn respond(&self, user_msg: &Value) -> GanyuResult<Value> {
        self.run(user_msg).await
    }

    /// 取出本次会话的推理轨迹（可观测 / 调试 / 续接展示）。
    pub fn trace(&self) -> Vec<Step> {
        self.steps.lock().unwrap().clone()
    }

    /// 续接会话：若记忆中存在该会话的轨迹，注入为开场上下文（跨重启自进化）。
    pub async fn resume(&self) -> bool {
        match self.memory.load_session(&self.session).await {
            Ok(Some(trace)) => {
                self.push(Step::Thought(format!(
                    "[续接会话 {}] 上次轨迹：{}",
                    self.session, trace
                )));
                true
            }
            _ => false,
        }
    }

    fn push(&self, s: Step) {
        self.steps.lock().unwrap().push(s);
    }
}

#[async_trait::async_trait]
impl Unit for Agent {
    fn name(&self) -> &str {
        if self.role.is_empty() {
            "agent"
        } else {
            &self.role
        }
    }

    /// 作为统一 `Unit` 运行：内部仍是 ReAct 循环；结果同时写入共享黑板（key=角色），
    /// 使它在 Blackboard / Graph 等编排里天然贡献到共享状态。
    async fn run(&self, ctx: &RunContext, input: &Value) -> GanyuResult<Value> {
        let out = self.run(input).await?;
        ctx.board_set(self.name(), out.clone());
        Ok(out)
    }
}
