//! Agent session and turn loop — the orchestration core of the crate.

pub use input::{TurnInputReceiver, TurnInputSender, turn_input_channel};
pub use session::AgentSession;
pub use turn::{TurnEvent, TurnResult};

mod compaction_phase;
mod hook_phase;
mod input;
mod memory_injection;
mod persistence;
mod provider_call;
mod session;
mod tool_phase;
mod turn;

pub use memory_injection::MemoryInjector;
