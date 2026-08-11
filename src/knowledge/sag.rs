//! 知识/分析面 · SAG 五步管道（用户自定义方法论）。
//!
//! 意图解析 → 上下文组装 → 生成+校验 → 执行 → 自进化写回
//!
//! - 生成：优先用 LLM 网关；网关仅本地兜底（返回非 SQL）时自动降级到模板生成器（自愈）。
//! - 校验：用 `Mdl::validate_sql` 做语义校验，失败则不执行，并尝试模板回退自愈。
//! - 自进化：成功路径经 `SkillBook` 写回 `agent/memory/cases`；用户纠正写回 `user/memory/preferences`。
//!   二次同类查询可命中历史（越用越懂业务）。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

use regex::Regex;

use crate::core::llm::Message;
use crate::core::memory::DynMemory;
use crate::error::GanyuResult;
use crate::ext::SkillBook;
use crate::knowledge::mdl::Mdl;
use crate::routing::Gateway;
use crate::session::SessionId;
use crate::value::Value;

/// 度量（Rust `enum`：类型安全，而非裸字符串）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Profit,
    Revenue,
    Cost,
}

impl Metric {
    fn expr(self) -> &'static str {
        match self {
            Metric::Profit => "SUM(s.revenue - s.cost - s.tax)",
            Metric::Revenue => "SUM(s.revenue)",
            Metric::Cost => "SUM(s.cost)",
        }
    }
    fn alias(self) -> &'static str {
        match self {
            Metric::Profit => "profit",
            Metric::Revenue => "revenue",
            Metric::Cost => "cost",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    LastMonth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
}

#[derive(Debug)]
pub struct Intent {
    pub raw: Value,
    pub metric: Metric,
    pub region: Option<String>,
    pub top_n: usize,
    pub period: Option<Period>,
}

pub struct SagOutput {
    pub sql: Value,
    pub result: Option<Value>,
    pub verdict: Verdict,
}

pub struct SagPipeline {
    pub mdl: Arc<Mdl>,
    pub gateway: Arc<Gateway>,
    pub memory: DynMemory,
    pub skills: Arc<SkillBook>,
    pub session: SessionId,
}

const ZONES: &[(&str, &str)] = &[
    ("华东", "华东"),
    ("华北", "华北"),
    ("华南", "华南"),
    ("华中", "华中"),
    ("西南", "西南"),
    ("东北", "东北"),
];

fn regex_contains(pat: &str, s: &str) -> bool {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    let re = guard
        .entry(pat.to_string())
        .or_insert_with(|| Regex::new(pat).unwrap());
    re.is_match(s)
}

fn capture_usize(pat: &str, s: &str) -> Option<usize> {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    let re = guard
        .entry(pat.to_string())
        .or_insert_with(|| Regex::new(pat).unwrap());
    re.captures(s)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<usize>().ok())
}

fn looks_like_sql(s: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\bSELECT\b").unwrap())
        .is_match(s)
}

fn parse_intent(query: &str) -> Intent {
    let metric = if regex_contains(r"(?i)营收|收入|revenue", query) {
        Metric::Revenue
    } else if regex_contains(r"(?i)成本|cost", query) {
        Metric::Cost
    } else {
        Metric::Profit
    };
    let region = ZONES.iter().find(|(zh, _)| query.contains(*zh)).map(|(_, z)| z.to_string());
    let top_n = capture_usize(r"(?:前|top)?\s*(\d+)\s*个", query).unwrap_or(3);
    let period = if regex_contains(r"(?i)上月|上个月|last month", query) {
        Some(Period::LastMonth)
    } else {
        None
    };
    Intent {
        raw: Value(query.to_string()),
        metric,
        region,
        top_n,
        period,
    }
}

fn template_sql(intent: &Intent) -> String {
    let expr = intent.metric.expr();
    let alias = intent.metric.alias();
    let mut sql = format!(
        "SELECT p.name, {expr} AS {alias}\nFROM sales s\nJOIN product p ON s.product_id = p.id\nJOIN region r ON s.region_id = r.id\n"
    );
    let mut conds = Vec::new();
    if let Some(z) = &intent.region {
        conds.push(format!("r.zone = '{z}'"));
    }
    if intent.period == Some(Period::LastMonth) {
        conds.push("s.period >= DATE_TRUNC('month', CURRENT_DATE - INTERVAL '1 month')".to_string());
    }
    if !conds.is_empty() {
        sql.push_str(&format!("WHERE {}\n", conds.join(" AND ")));
    }
    sql.push_str(&format!("GROUP BY p.name\nORDER BY {alias} DESC\nLIMIT {}", intent.top_n));
    sql
}

/// 执行步骤（演示用 mock 结果；接真实库时替换此函数）。
fn execute_mock() -> Value {
    Value(
        serde_json::json!([
            ["产品A", 123456],
            ["产品B", 98765],
            ["产品C", 87654]
        ])
        .to_string(),
    )
}

impl SagPipeline {
    pub async fn run(&self, query: &Value, corrected: Option<&Value>) -> GanyuResult<SagOutput> {
        let intent = parse_intent(query.as_str());

        // Step 1+2：意图 + 上下文（记忆检索）
        let ctx = self.memory.search(query.as_str(), "viking://").await?;
        let ctx_str = ctx
            .iter()
            .map(|h| format!("- {}: {}", h.uri, h.l0))
            .collect::<Vec<_>>()
            .join("\n");

        // Step 3：生成 + 校验（自愈：网关失败/非 SQL → 模板降级）
        let prompt = format!(
            "你是 Text-to-SQL 助手。基于 MDL 生成 SQL。\nMDL 约束: profit=SUM(revenue-cost-tax); sales↔product(MANY_TO_ONE); sales↔region(MANY_TO_ONE)\n上下文:\n{ctx_str}\n问题: {}",
            query
        );
        let mut sql = match self.gateway.complete(&[Message::user(prompt)]).await {
            Ok(v) if looks_like_sql(v.as_str()) => v.as_str().to_string(),
            _ => template_sql(&intent),
        };

        let (ok, problems) = self.mdl.validate_sql(&sql);
        let mut verdict = if ok { Verdict::Pass } else { Verdict::Fail };

        if !ok {
            // 自愈：校验失败，回退模板 SQL 再校验一次
            sql = template_sql(&intent);
            let (ok2, problems2) = self.mdl.validate_sql(&sql);
            if ok2 {
                verdict = Verdict::Pass;
            } else {
                self.skills
                    .heal_from_failure(query.as_str(), &format!("{problems2:?}"))
                    .await?;
            }
            let _ = problems; // 首次问题已用于触发自愈
        }

        // Step 4：执行
        let result = if verdict == Verdict::Pass {
            Some(execute_mock())
        } else {
            None
        };

        // Step 5：自进化写回
        if let Some(corr) = corrected {
            self.memory
                .put("viking://user/memory/preferences/profit_definition", corr)
                .await?;
            self.memory
                .put("viking://agent/memory/cases/correction_profit", corr)
                .await?;
        } else if verdict == Verdict::Pass {
            if let Some(r) = &result {
                self.skills.capture(query.as_str(), "sag_top3_profit", r).await?;
            }
        }

        // 提交会话轨迹（记忆层自愈）
        let trace = Value(
            serde_json::json!({
                "session": self.session.as_string(),
                "intent": format!("{intent:?}"),
                "verdict": format!("{verdict:?}"),
            })
            .to_string(),
        );
        self.memory.commit(&self.session, &trace).await?;

        Ok(SagOutput { sql: Value(sql), result, verdict })
    }
}
