use crate::config::{AgentConfig, TaskPath};
use crate::error::AgentError;
use crate::model::message::Message;
use crate::model::provider::ModelHint;

use super::{Pass, PassControl};

/// Mutable control state shared by the passes of one turn.
#[derive(Debug, Clone)]
pub struct TurnState {
    turn_id: u64,
    current_pass: Pass,
    iteration: usize,
    previous_tool_error: bool,
    consecutive_noop: usize,
    force_thinking: bool,
}

impl TurnState {
    pub fn new(turn_id: u64) -> Self {
        Self {
            turn_id,
            current_pass: Pass::BeginTurn,
            iteration: 0,
            previous_tool_error: false,
            consecutive_noop: 0,
            force_thinking: false,
        }
    }

    pub fn enter(&mut self, pass: Pass) {
        let previous = self.current_pass;
        self.apply(PassControl::Next(pass));
        tracing::trace!(
            turn_id = self.turn_id,
            from = ?previous,
            to = ?self.current_pass,
            "agent pass transition"
        );
    }

    pub fn iteration(&self) -> usize {
        self.iteration
    }

    fn apply(&mut self, control: PassControl) -> bool {
        match control {
            PassControl::Next(pass) => self.current_pass = pass,
            PassControl::Complete => return false,
        }
        true
    }

    pub fn begin_iteration(&mut self, max_iterations: Option<usize>) -> Result<usize, AgentError> {
        if let Some(max) = max_iterations
            && self.iteration >= max
        {
            return Err(AgentError::MaxIterations(max));
        }
        self.iteration += 1;
        Ok(self.iteration)
    }

    pub fn request_thinking(&mut self) {
        self.force_thinking = true;
    }

    pub fn model_hint(&mut self, config: &AgentConfig) -> ModelHint {
        if self.force_thinking {
            self.force_thinking = false;
            return ModelHint::Thinking;
        }
        if config.path == TaskPath::Fast {
            ModelHint::Execution
        } else if self.previous_tool_error {
            ModelHint::Recovery
        } else if self.consecutive_noop >= 3 || self.iteration == 1 {
            ModelHint::Thinking
        } else if config.path == TaskPath::Heavy && self.iteration.is_multiple_of(4) {
            ModelHint::Thinking
        } else {
            ModelHint::Execution
        }
    }

    pub fn observe_assistant(&mut self, message: &Message) {
        let has_tool_calls = message.tool_calls().next().is_some();
        if has_tool_calls && message.text_content().is_empty() {
            self.consecutive_noop += 1;
        } else if has_tool_calls {
            self.consecutive_noop = 0;
        }
    }

    pub fn observe_tool_results(&mut self, message: &Message) {
        self.previous_tool_error = message.tool_results_iter().any(|result| result.is_error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::message::{ContentBlock, Role, ToolResult};

    #[test]
    fn enforces_iteration_limit_before_incrementing() {
        let mut state = TurnState::new(7);
        assert_eq!(state.begin_iteration(Some(1)).unwrap(), 1);
        assert!(matches!(state.begin_iteration(Some(1)), Err(AgentError::MaxIterations(1))));
        assert_eq!(state.iteration, 1);
    }

    #[test]
    fn forced_thinking_is_consumed_once() {
        let mut state = TurnState::new(7);
        state.begin_iteration(None).unwrap();
        state.request_thinking();
        let config = AgentConfig { path: TaskPath::Fast, ..AgentConfig::default() };
        assert_eq!(state.model_hint(&config), ModelHint::Thinking);
        assert_eq!(state.model_hint(&config), ModelHint::Execution);
    }

    #[test]
    fn tool_error_selects_recovery_hint() {
        let mut state = TurnState::new(7);
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
