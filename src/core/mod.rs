//! 抽象层聚合：llm / memory / agent / loop。

pub mod agent;
pub mod llm;
pub mod loop_;
pub mod memory;

pub use agent::Agent;
pub use llm::{DynBackend, LocalBackend, LlmBackend, Message, Role};
#[cfg(feature = "network")]
pub use llm::OpenAiBackend;
pub use loop_::{Decision, LocalReasoner, Reasoner, Step};
pub use memory::{DynMemory, LocalMemory, Memory, MemoryHit, OpenVikingMemory};
