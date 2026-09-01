//! 抽象层聚合：llm / memory / agent / loop / unit / workflow。

pub mod agent;
pub mod llm;
pub mod loop_;
pub mod memory;
pub mod unit;
pub mod workflow;

pub use agent::Agent;
#[cfg(feature = "network")]
pub use llm::OpenAiBackend;
pub use llm::{DynBackend, LlmBackend, LocalBackend, Message, Role};
pub use loop_::{Decision, LocalReasoner, Reasoner, Step};
pub use memory::{DynMemory, LocalMemory, Memory, MemoryHit, OpenVikingMemory};
pub use unit::{RunContext, Unit};
