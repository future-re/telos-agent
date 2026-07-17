//! Agent lifecycle, context management, prompting, policies, and turn execution.

pub mod compaction;
pub mod context;
pub mod policies;
pub mod prompt;
pub mod runtime;
pub mod turn;

pub use context::{MemoryInjector, SkillInjector};
pub use runtime::{AgentRuntime, AgentSession, TurnHandle};
pub use turn::{TurnEvent, TurnInputReceiver, TurnInputSender, TurnResult, turn_input_channel};
