//! Agent session and turn loop — the orchestration core of the crate.

pub use input::{TurnInputReceiver, TurnInputSender, turn_input_channel};
pub use memory_injection::MemoryInjector;
pub use session::AgentSession;
pub use skill_injection::SkillInjector;
pub use turn::{TurnEvent, TurnResult};

mod compaction_phase;
mod hook_phase;
mod input;
mod memory_injection;
mod persistence;
mod provider_call;
mod session;
mod skill_injection;
mod tool_phase;
mod turn;
