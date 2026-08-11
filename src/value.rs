//! 统一数据类型：`Value` 即 `String`。
//!
//! 全链路（消息内容、记忆值、工具输入输出、生成的 SQL）都收敛为字符串，
//! 通过 `From` 实现把任意基础类型提升为 `Value`，把 `Value` 降为 `String`。

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub struct Value(pub String);

impl Value {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value(s)
    }
}

impl From<&String> for Value {
    fn from(s: &String) -> Self {
        Value(s.clone())
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value(n.to_string())
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value(n.to_string())
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value(b.to_string())
    }
}

impl From<Value> for String {
    fn from(v: Value) -> Self {
        v.0
    }
}
