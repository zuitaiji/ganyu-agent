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

        // H2：注入防护——任何疑似注入特征都计入问题，使校验失败。
        problems.extend(self.detect_injection(sql));

        (problems.is_empty(), problems)
    }

    /// H2：SQL 注入特征检测。返回命中的风险描述列表（空=未命中）。
    ///
    /// 防御重点：堆叠语句、注释截断、危险 DML/DDL、UNION 注入、系统函数等。
    /// 注意：这是**生成侧**的兜底；真正的强隔离应在执行层用参数化查询（Prepared Statement），
    /// 本系统的 `template_sql` 已用白名单区域值 + 数值 `top_n` 构造，天然规避拼接注入。
    pub fn detect_injection(&self, sql: &str) -> Vec<String> {
        let mut hits = Vec::new();
        let lower = sql.to_ascii_lowercase();

        // 注释/堆叠语句截断
        if lower.contains("--") || lower.contains("#") || lower.contains("/*") || lower.contains("*/") {
            hits.push("含注释或堆叠语句截断（-- / # / /* */）".into());
        }
        if lower.contains(';') {
            hits.push("含多条语句分隔符（;），疑似堆叠查询".into());
        }

        // 危险关键字（DML/DDL/系统函数）
        const DANGER: &[&str] = &[
            "drop", "delete", "update", "insert", "alter", "truncate", "create",
            "replace", "grant", "revoke", "exec", "execute", "union", "into",
            "xp_", "sleep", "benchmark", "load_file", "outfile", "information_schema",
        ];
        for kw in DANGER {
            // 词边界匹配，避免误伤正常列名（如 `updated_at` 不应触发 `update`）。
            let pat = format!(r"(?i)(^|[^a-z0-9_]){kw}([^a-z0-9_]|$)");
            if let Ok(re) = regex::Regex::new(&pat) {
                if re.is_match(&lower) {
                    hits.push(format!("含危险关键字：{kw}"));
                }
            }
        }
        hits
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Mdl {
        // 复用 examples 的结构（这里内联最小模型用于单测）。
        let doc = r#"{"models":[{"name":"sales","columns":[{"name":"revenue"},{"name":"cost"}]}]}"#;
        let d: MdlDoc = serde_json::from_str(doc).unwrap();
        Mdl { models: d.models.into_iter().map(|m| (m.name.clone(), m)).collect() }
    }

    #[test]
    fn valid_sql_passes() {
        let m = sample();
        let (ok, p) = m.validate_sql("SELECT revenue FROM sales");
        assert!(ok, "problems: {p:?}");
    }

    #[test]
    fn injection_detected() {
        let m = sample();
        let (ok, p) = m.validate_sql("SELECT revenue FROM sales; DROP TABLE sales--");
        assert!(!ok);
        assert!(p.iter().any(|x| x.contains("堆叠") || x.contains("注释") || x.contains("DROP")));
    }

    #[test]
    fn unknown_table_flagged() {
        let m = sample();
        let (ok, p) = m.validate_sql("SELECT x FROM nonexistent");
        assert!(!ok);
        assert!(p.iter().any(|x| x.contains("未知表")));
    }
}
