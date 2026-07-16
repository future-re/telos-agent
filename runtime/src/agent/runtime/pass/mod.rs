//! State and control-flow definitions for the agent runtime pipeline.
//!
//! A pass is an execution stage. Events describe what happened during a pass;
//! they do not select or execute the next stage.

mod state;

mod compaction;
mod hooks;
mod injection;
mod provider;
pub(crate) mod runner;
mod tools;
mod util;

pub use state::TurnState;

/// Stable stages in one agent turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    BeginTurn,
    BeginIteration,
    DrainInput,
    CompactContext,
    InjectContext,
    CallProvider,
    PostSamplingHooks,
    RouteAssistant,
    ExecuteTools,
    StopHooks,
    PersistTurn,
    FinishTurn,
}

/// Control instruction returned by a pass executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassControl {
    Next(Pass),
    Complete,
}

const MAX_CONSECUTIVE_COMPACTION_FAILURES: usize = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_control_keeps_transition_explicit() {
        assert_eq!(PassControl::Next(Pass::CallProvider), PassControl::Next(Pass::CallProvider));
        assert_ne!(PassControl::Next(Pass::CallProvider), PassControl::Complete);
    }
}
