//! ganyu-agent：有温度、能自进化、可拓展、可自愈的完备 agent 系统（Rust）。
//!
//! 设计硬约束：
//! - 会话 UUID（`SessionId`）贯穿每一次交互与记忆提交。
//! - 统一数据类型为字符串（`Value`），所有载荷均收敛为 `String`。
//! - 抽象层：`Memory` / `LlmBackend` / `Tool` 三个 trait + `Gateway`(路由) + `Agent`(编排) + `heal`(自愈)。
//! - 外部重服务走适配器 + 本地降级（自愈），默认零网络依赖即可编译运行。

pub mod cache;
pub mod config;
pub mod core;
pub mod error;
pub mod ext;
pub mod heal;
pub mod knowledge;
pub mod observe;
pub mod persona;
pub mod routing;
pub mod sandbox;
pub mod security;
pub mod session;
pub mod release_sign;
pub mod tools;
pub mod value;

pub use error::{GanyuError, GanyuResult};
pub use session::SessionId;
pub use value::Value;
