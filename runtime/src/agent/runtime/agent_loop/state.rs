//! State retained between model iterations in a single turn.

use crate::{AgentConfig, AgentError, Message, ModelHint, TaskPath};

/// Mutable signals that influence the next model iteration.
///
/// This is deliberately not a control-flow state machine. The control flow is
/// visible in `turn::run_turn`; this type only tracks data that survives
/// across iterations.
pub(super) struct LoopState {
    pending_feedback: Vec<String>,
    iteration: usize,
    previous_tool_error: bool,
    consecutive_tool_only_responses: usize,
    force_thinking: bool,
}

impl LoopState {
    pub(super) fn new() -> Self {
        Self {
            pending_feedback: Vec::new(),
            iteration: 0,
            previous_tool_error: false,
            consecutive_tool_only_responses: 0,
            force_thinking: false,
        }
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

    pub(super) fn request_thinking(&mut self) {
        self.force_thinking = true;
    }

    pub(super) fn model_hint(&mut self, config: &AgentConfig) -> ModelHint {
        if self.force_thinking {
            self.force_thinking = false;
            return ModelHint::Thinking;
        }
        if config.path == TaskPath::Fast {
            ModelHint::Execution
        } else if self.previous_tool_error {
            ModelHint::Recovery
        } else if self.consecutive_tool_only_responses >= 3
            || self.iteration == 1
            || (config.path == TaskPath::Heavy && self.iteration.is_multiple_of(4))
        {
            ModelHint::Thinking
        } else {
            ModelHint::Execution
        }
    }

    pub(super) fn observe_assistant(&mut self, message: &Message) {
        let calls = message.tool_calls().next().is_some();
        if calls && message.text_content().is_empty() {
            self.consecutive_tool_only_responses += 1;
        } else {
            self.consecutive_tool_only_responses = 0;
        }
    }

    pub(super) fn observe_tool_results(&mut self, message: &Message) {
        self.previous_tool_error = message.tool_results_iter().any(|result| result.is_error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::message::{ContentBlock, Role, ToolResult};

    #[test]
    fn enforces_iteration_limit_before_incrementing() {
        let mut state = LoopState::new();
        assert_eq!(state.begin_iteration(Some(1)).unwrap(), 1);
        assert!(matches!(state.begin_iteration(Some(1)), Err(AgentError::MaxIterations(1))));
    }

    #[test]
    fn forced_thinking_hint_is_consumed_once() {
        let mut state = LoopState::new();
        state.begin_iteration(None).unwrap();
        state.request_thinking();
        let config = AgentConfig { path: TaskPath::Fast, ..AgentConfig::default() };

        assert_eq!(state.model_hint(&config), ModelHint::Thinking);
        assert_eq!(state.model_hint(&config), ModelHint::Execution);
    }

    #[test]
    fn tool_error_selects_recovery_hint() {
        let mut state = LoopState::new();
        state.begin_iteration(None).unwrap();
        state.observe_tool_results(&Message {
            role: Role::Tool,
            blocks: vec![ContentBlock::ToolResult(ToolResult {
                tool_call_id: "call-1".into(),
                name: "test".into(),
                content: serde_json::json!({}),
                is_error: true,
            })],
        });

        assert_eq!(state.model_hint(&AgentConfig::default()), ModelHint::Recovery);
    }
}
