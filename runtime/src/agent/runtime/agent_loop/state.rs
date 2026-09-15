//! State retained between model iterations in a single turn.

use crate::{AgentError, ModelHint};

/// Mutable signals that influence the next model iteration.
///
/// This is deliberately not a control-flow state machine. The control flow is
/// visible in `turn::run_turn`; this type only tracks data that survives
/// across iterations.
pub(super) struct LoopState {
    pending_feedback: Vec<String>,
    iteration: usize,
}

impl LoopState {
    pub(super) fn new() -> Self {
        Self { pending_feedback: Vec::new(), iteration: 0 }
    }

    pub(super) fn queue_feedback(&mut self, feedback: impl IntoIterator<Item = String>) {
        self.pending_feedback.extend(feedback);
    }

    pub(super) fn take_feedback(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_feedback)
    }

    pub(super) fn begin_iteration(&mut self, max: Option<usize>) -> Result<usize, AgentError> {
        if let Some(max) = max
            && self.iteration >= max
        {
            return Err(AgentError::MaxIterations(max));
        }
        self.iteration += 1;
        Ok(self.iteration)
    }

    pub(super) fn model_hint(&self) -> ModelHint {
        ModelHint::Execution
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enforces_iteration_limit_before_incrementing() {
        let mut state = LoopState::new();
        assert_eq!(state.begin_iteration(Some(1)).unwrap(), 1);
        assert!(matches!(state.begin_iteration(Some(1)), Err(AgentError::MaxIterations(1))));
    }

    #[test]
    fn default_model_hint_is_execution() {
        let mut state = LoopState::new();
        state.begin_iteration(None).unwrap();
        assert_eq!(state.model_hint(), ModelHint::Execution);
    }
}
