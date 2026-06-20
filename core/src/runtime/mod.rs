//! Agent session and turn loop — the orchestration core of the crate.

pub use session::AgentSession;
pub use turn::{TurnEvent, TurnResult};

mod compaction_phase;
mod hook_phase;
mod persistence;
mod provider_call;
mod session;
mod tool_phase;
mod turn;
