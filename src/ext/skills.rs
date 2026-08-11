//! 技能框架（可生长的特性能力）。
//!
//! 与原子 `Tool` 不同，`Skill` 是**多步复合程序**：按顺序执行若干步骤（调用其他工具、
//! 离线推理、摘要），产出结果。技能被包装成 `SkillTool` 以 `skill:<name>` 名字注册进
//! 工具表，于是它既能 `@skill:summarize x.txt` 直接调用，也能被推理循环自动路由命中。
//!
//! 这是「生长出特色技能」的机制：内置 3 个（summarize / troubleshoot / kb_query），
//! 后续可 `SkillBook::register_skill` 注入业务专属技能，无需改核心代码。

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::GanyuResult;
use crate::ext::{SkillBook, Tool, ToolRegistry};
use crate::value::Value;

/// 技能的一个步骤。
#[derive(Debug, Clone)]
pub enum SkillStep {
    /// 调用某个已注册工具，`arg` 中的 `{input}` 会被替换为技能输入。
    Call { tool: String, arg: String },
    /// 离线固定说明（作为 Observation 回流）。
    Note { text: String },
    /// 对上一步 Observation 做离线摘要（行数/字符数 + 前 N 字符）。
    Summarize { max_chars: usize },
}

/// 一个可生长的特性技能。
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub steps: Vec<SkillStep>,
}

impl Skill {
    fn render(&self, arg: &str, input: &str) -> String {
        arg.replace("{input}", input)
    }
}

/// 把 `Skill` 包装成 `Tool`，以 `skill:<name>` 注册，复用工具表的调度与重试。
pub struct SkillTool {
    book: Arc<SkillBook>,
    tools: Arc<ToolRegistry>,
    /// 技能名（用于查 `SkillBook`）。
    skill_name: String,
    /// 工具对外名：`skill:{skill_name}`。
    tool_name: String,
    description: String,
}

impl SkillTool {
    pub fn new(
        book: Arc<SkillBook>,
        tools: Arc<ToolRegistry>,
        skill_name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let skill_name = skill_name.into();
        SkillTool {
            book,
            tools,
            tool_name: format!("skill:{skill_name}"),
            skill_name,
            description: description.into(),
        }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        &self.tool_name
    }
    fn description(&self) -> &str {
        &self.description
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        let skill = self
            .book
            .get_skill(&self.skill_name)
            .ok_or_else(|| crate::error::GanyuError::ToolNotFound(format!("skill:{}", self.skill_name)))?;
        let mut last = Value::default();
        for step in &skill.steps {
            match step {
                SkillStep::Call { tool, arg } => {
                    let rendered = skill.render(arg, input.as_str());
                    last = self
                        .tools
                        .call(tool, &Value(rendered))
                        .await
                        .unwrap_or_else(|e| Value(format!("[技能步骤 {tool} 失败：{e}]")));
                }
                SkillStep::Note { text } => {
                    last = Value(text.clone());
                }
                SkillStep::Summarize { max_chars } => {
                    let s = last.as_str();
                    let lines = s.lines().count();
                    let chars = s.chars().count();
                    let head: String = s.chars().take(*max_chars).collect();
                    last = Value(format!(
                        "摘要：{lines} 行 / {chars} 字符；前 {max_chars} 字符：{head}"
                    ));
                }
            }
        }
        Ok(last)
    }
}

/// 注册内置特性技能（离线即可用，体现「特色」）。
pub fn register_core_skills(book: &SkillBook) {
    book.register_skill(Skill {
        name: "summarize".into(),
        description: "读取文件并给出离线摘要（行数/字符数 + 前若干字符）".into(),
        steps: vec![
            SkillStep::Call {
                tool: "file_read".into(),
                arg: "{input}".into(),
            },
            SkillStep::Summarize { max_chars: 120 },
        ],
    });
    book.register_skill(Skill {
        name: "troubleshoot".into(),
        description: "根据报错/现象检索记忆中的成功案例与失败沉淀，给出排查指引".into(),
        steps: vec![SkillStep::Call {
            tool: "rag_search".into(),
            arg: "{input}".into(),
        }],
    });
    book.register_skill(Skill {
        name: "kb_query".into(),
        description: "向记忆知识库提问（检索 + 摘要）".into(),
        steps: vec![
            SkillStep::Call {
                tool: "rag_search".into(),
                arg: "{input}".into(),
            },
            SkillStep::Summarize { max_chars: 200 },
        ],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::memory::LocalMemory;

    #[tokio::test]
    async fn summarize_skill_reads_and_summarizes() {
        let mem: Arc<dyn crate::core::memory::Memory + Send + Sync> =
            Arc::new(LocalMemory::new(".ganyu_skill_test_mem.json"));
        let tools = Arc::new(ToolRegistry::new());
        crate::ext::builtins::register_core_tools(&tools, mem.clone());
        let book = Arc::new(SkillBook::new(mem));
        register_core_skills(&book);
        // 注册技能工具
        for name in book.skill_names() {
            let desc = book
                .get_skill(&name)
                .map(|s| s.description.clone())
                .unwrap_or_default();
            tools.register(Arc::new(SkillTool::new(
                book.clone(),
                tools.clone(),
                name.clone(),
                desc,
            )));
        }
        let p = ".ganyu_skill_test.txt";
        let _ = std::fs::remove_file(p);
        tools
            .call("file_write", &Value(format!("{p}\nline1\nline2\nline3")))
            .await
            .unwrap();
        let out = tools.call("skill:summarize", &Value(p.into())).await.unwrap();
        assert!(out.as_str().contains("摘要"));
        assert!(out.as_str().contains("3 行"));
        let _ = std::fs::remove_file(p);
        let _ = std::fs::remove_file(".ganyu_skill_test_mem.json");
    }
}
