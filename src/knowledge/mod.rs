//! 知识/分析面：MDL 语义骨架 + SAG 管道。

pub mod mdl;
pub mod sag;

pub use mdl::Mdl;
pub use sag::{Intent, SagOutput, SagPipeline, Verdict};
