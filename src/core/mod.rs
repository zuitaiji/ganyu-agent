//! 抽象层聚合：llm / memory / agent。

pub mod agent;
pub mod llm;
pub mod memory;

pub use agent::Agent;
pub use llm::{DynBackend, LocalBackend, LlmBackend, Message, Role};
#[cfg(feature = "network")]
pub use llm::OpenAiBackend;
pub use memory::{DynMemory, LocalMemory, Memory, MemoryHit, OpenVikingMemory};
