mod call_provider;
mod compaction_phase;
mod hook_phase;
mod injection_phase;
pub(crate) mod run_stream;
mod run_turn;
mod tool_phase;
mod util;

pub use run_turn::run_turn;

#[cfg(not(feature = "turn-stream"))]
pub use run_turn::run_turn as agent_run;

#[cfg(feature = "turn-stream")]
pub use run_stream::run_turn_stream as agent_run;

/// After this many consecutive compaction failures, stop trying.
const MAX_CONSECUTIVE_COMPACTION_FAILURES: usize = 3;
