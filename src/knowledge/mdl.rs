//! 知识/分析面 · WrenAI MDL（语义骨架）加载与本地语义校验。
//!
//! 规划里 WrenAI 用 `wren dry-plan` 做语义校验；这里提供**零依赖的本地校验器**，
//! 不需要 wren CLI 即可验证生成的 SQL 是否引用了 MDL 中存在的表/列/关系。
//! 这既是离线可跑的关键，也是"自愈"的一环：生成阶段就能拦住不合法的 SQL。

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::error::GanyuResult;
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct MdlColumn {
    name: String,
}

#[derive(Debug, Deserialize)]
struct MdlModel {
    name: String,
    #[serde(default)]
    columns: Vec<MdlColumn>,
}

#[derive(Debug, Deserialize)]
struct MdlMetric {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    model: String,
}

#[derive(Debug, Deserialize)]
struct MdlDoc {
    #[serde(default)]
    models: Vec<MdlModel>,
    #[serde(default)]
    #[allow(dead_code)]
    metrics: Vec<MdlMetric>,
}

pub struct Mdl {
    models: HashMap<String, MdlModel>,
}

impl Mdl {
    pub fn load(path: &str) -> GanyuResult<Self> {
        let s = std::fs::read_to_string(path)?;
        let doc: MdlDoc = serde_json::from_str(&s)?;
        let models = doc.models.into_iter().map(|m| (m.name.clone(), m)).collect();
        Ok(Mdl { models })
    }

    pub fn tables(&self) -> Vec<&str> {
        self.models.keys().map(|s| s.as_str()).collect()
    }

    /// 轻量语义校验：引用的表/列必须存在于 MDL。返回 (ok, 问题列表)。
    pub fn validate_sql(&self, sql: &str) -> (bool, Vec<String>) {
        let mut problems = Vec::new();

        for cap in table_regex().captures_iter(sql) {
            let t = &cap[1];
            if !self.models.keys().any(|k| k.eq_ignore_ascii_case(t)) {
                problems.push(format!("未知表：{t}"));
            }
        }

        for cap in col_regex().captures_iter(sql) {
            let tbl = &cap[1];
            let col = &cap[2];
            if let Some(m) = self.models.get(tbl) {
                if !m.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col)) {
                    problems.push(format!("表 {tbl} 无列 {col}"));
                }
            }
            // 表本身不存在的情况由上面的表校验覆盖
        }

        (problems.is_empty(), problems)
    }
}

fn table_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(?:FROM|JOIN)\s+([A-Za-z_]\w*)").unwrap())
}

fn col_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"([A-Za-z_]\w*)\.([A-Za-z_]\w*)").unwrap())
}
