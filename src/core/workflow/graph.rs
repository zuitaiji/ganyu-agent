//! Graph Workflow 范式：把节点（Unit / 常量）连成 DAG，按拓扑序执行，边传递数据。
//!
//! - 节点：`.node(id, Unit)` 或 `.literal(id, text)`（常量输入）。
//! - 边：`.edge(from, to)`；目标节点接收其所有前驱输出的合并值。
//! - 入口：无前驱的节点直接收到工作流输入 `input`。
//! - 出口：`.end(id)` 指定最终节点，其输出即工作流结果。
//!
//! 对齐可视化编排（LangGraph / 工作流引擎）：显式控制流 + 数据依赖，离线可跑。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;

use crate::core::unit::{RunContext, Unit};
use crate::error::{GanyuError, GanyuResult};
use crate::value::Value;

use super::Workflow;

enum Node {
    Unit(Arc<dyn Unit>),
    Literal(Value),
}

pub struct GraphWorkflow {
    nodes: HashMap<String, Node>,
    edges: Vec<(String, String)>,
    end: String,
}

impl GraphWorkflow {
    pub fn builder() -> GraphBuilder {
        GraphBuilder::default()
    }

    fn predecessors(&self, id: &str) -> Vec<String> {
        self.edges
            .iter()
            .filter(|(_, to)| to == id)
            .map(|(from, _)| from.clone())
            .collect()
    }

    /// 拓扑序（Kahn）。若有环返回错误。
    fn topo_order(&self) -> GanyuResult<Vec<String>> {
        let mut indeg: HashMap<String, usize> = self
            .nodes
            .keys()
            .map(|k| (k.clone(), 0))
            .collect();
        for (_, to) in &self.edges {
            *indeg.entry(to.clone()).or_insert(0) += 1;
        }
        let mut q: VecDeque<String> = indeg
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(k, _)| k.clone())
            .collect();
        let mut order = Vec::new();
        while let Some(n) = q.pop_front() {
            order.push(n.clone());
            for (from, to) in &self.edges {
                if from == &n {
                    let d = indeg.get_mut(to).unwrap();
                    *d -= 1;
                    if *d == 0 {
                        q.push_back(to.clone());
                    }
                }
            }
        }
        if order.len() != self.nodes.len() {
            return Err(crate::error::GanyuError::Workflow(
                "graph 存在环或孤立节点".into(),
            ));
        }
        Ok(order)
    }
}

#[async_trait]
impl Workflow for GraphWorkflow {
    fn mode(&self) -> &str {
        "graph"
    }

    async fn run(&self, ctx: &RunContext, input: &Value) -> GanyuResult<Value> {
        let order = self.topo_order()?;
        let mut outputs: HashMap<String, Value> = HashMap::new();
        for id in &order {
            // 收集前驱输出
            let preds = self.predecessors(id);
            let mut collected: Vec<Value> = Vec::new();
            for p in &preds {
                if let Some(v) = outputs.get(p) {
                    collected.push(v.clone());
                }
            }
            let node_input: Value = match collected.len() {
                0 => input.clone(), // 入口节点
                1 => collected.into_iter().next().unwrap(),
                _ => Value(collected.iter().map(|v| v.as_str()).collect::<Vec<_>>().join("\n")),
            };

            let out = match self.nodes.get(id).unwrap() {
                Node::Unit(u) => u.run(ctx, &node_input).await?,
                Node::Literal(v) => v.clone(),
            };
            outputs.insert(id.clone(), out);
        }
        outputs
            .get(&self.end)
            .cloned()
            .ok_or_else(|| crate::error::GanyuError::Workflow(format!("end 节点缺失：{}", self.end)))
    }
}

/// 流式构造 Graph 的构建器。
pub struct GraphBuilder {
    nodes: HashMap<String, Node>,
    edges: Vec<(String, String)>,
    end: String,
}

impl Default for GraphBuilder {
    fn default() -> Self {
        GraphBuilder {
            nodes: HashMap::new(),
            edges: Vec::new(),
            end: String::new(),
        }
    }
}

impl GraphBuilder {
    pub fn node(mut self, id: &str, unit: Arc<dyn Unit>) -> Self {
        self.nodes.insert(id.to_string(), Node::Unit(unit));
        self
    }

    pub fn literal(mut self, id: &str, text: &str) -> Self {
        self.nodes.insert(id.to_string(), Node::Literal(Value(text.into())));
        self
    }

    pub fn edge(mut self, from: &str, to: &str) -> Self {
        self.edges.push((from.to_string(), to.to_string()));
        self
    }

    pub fn end(mut self, id: &str) -> Self {
        self.end = id.to_string();
        self
    }

    pub fn build(self) -> GanyuResult<GraphWorkflow> {
        if self.end.is_empty() {
            return Err(GanyuError::Workflow("未指定 end 节点".into()));
        }
        if !self.nodes.contains_key(&self.end) {
            return Err(GanyuError::Workflow(format!("end 节点不存在：{}", self.end)));
        }
        // 构造即校验：有环或存在孤立/不可达节点则拒绝。
        self.validate()?;
        Ok(GraphWorkflow {
            nodes: self.nodes,
            edges: self.edges,
            end: self.end,
        })
    }

    /// Kahn 入度检查：边指向缺失节点 / 成环 / 不可达 均报错。
    fn validate(&self) -> GanyuResult<()> {
        let mut indeg: HashMap<&String, usize> =
            self.nodes.keys().map(|k| (k, 0)).collect();
        for (_, to) in &self.edges {
            if let Some(d) = indeg.get_mut(to) {
                *d += 1;
            } else {
                return Err(GanyuError::Workflow(format!("边指向不存在的节点：{to}")));
            }
        }
        let mut q: VecDeque<&String> = indeg
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(k, _)| *k)
            .collect();
        let mut count = 0;
        while let Some(n) = q.pop_front() {
            count += 1;
            for (from, to) in &self.edges {
                if from == n {
                    if let Some(d) = indeg.get_mut(to) {
                        *d -= 1;
                        if *d == 0 {
                            q.push_back(to);
                        }
                    }
                }
            }
        }
        if count != self.nodes.len() {
            return Err(GanyuError::Workflow("graph 存在环或孤立节点".into()));
        }
        Ok(())
    }
}
