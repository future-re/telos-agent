//! State and control-flow definitions for the agent runtime pipeline.
//!
//! A pass is an execution stage. Events describe what happened during a pass;
//! they do not select or execute the next stage.

mod compaction;
mod injection;
mod provider;
pub(crate) mod runner;
mod tools;
mod util;

/// Private effects requested by the turn state machine and executed by the driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Effect {
    BeginTurn,
    BeginIteration,
    DrainInput,
    CompactContext,
    InjectContext,
    CallProvider,
    EvaluateModelPolicies,
    RouteAssistant,
    ExecuteTools,
    ApplyFeedback,
    EvaluateFinishPolicies,
    PersistTurn,
    FinishTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectResult {
    Done,
    Compaction { abort: bool },
    ModelRouted { has_tools: bool },
    FeedbackApplied { had_feedback: bool },
    FinishPolicies { has_feedback: bool },
}

struct TurnMachine {
    effect: Effect,
    feedback: Vec<String>,
    turn_id: u64,
    iteration: usize,
    previous_tool_error: bool,
    consecutive_noop: usize,
    force_thinking: bool,
}

impl TurnMachine {
    fn new(turn_id: u64) -> Self {
        Self {
            effect: Effect::BeginTurn,
            feedback: Vec::new(),
            turn_id,
            iteration: 0,
            previous_tool_error: false,
            consecutive_noop: 0,
            force_thinking: false,
        }
    }
    fn effect(&self) -> Effect {
        self.effect
    }
    fn transition(&mut self, next: Effect) {
        tracing::trace!(turn_id = self.turn_id, from = ?self.effect, to = ?next, "turn effect transition");
        self.effect = next;
    }
    fn advance(&mut self, result: EffectResult) -> Result<Option<Effect>, crate::AgentError> {
        let next = match (self.effect, result) {
            (Effect::BeginTurn, EffectResult::Done) => Some(Effect::BeginIteration),
            (Effect::BeginIteration, EffectResult::Done) => Some(Effect::DrainInput),
            (Effect::DrainInput, EffectResult::Done) => Some(Effect::CompactContext),
            (Effect::CompactContext, EffectResult::Compaction { abort: false }) => {
                Some(Effect::InjectContext)
            }
            (Effect::CompactContext, EffectResult::Compaction { abort: true }) => {
                Some(Effect::PersistTurn)
            }
            (Effect::InjectContext, EffectResult::Done) => Some(Effect::CallProvider),
            (Effect::CallProvider, EffectResult::Done) => Some(Effect::EvaluateModelPolicies),
            (Effect::EvaluateModelPolicies, EffectResult::Done) => Some(Effect::RouteAssistant),
            (Effect::RouteAssistant, EffectResult::ModelRouted { has_tools: true }) => {
                Some(Effect::ExecuteTools)
            }
            (Effect::RouteAssistant, EffectResult::ModelRouted { has_tools: false }) => {
                Some(Effect::ApplyFeedback)
            }
            (Effect::ExecuteTools, EffectResult::Done) => Some(Effect::ApplyFeedback),
            (Effect::ApplyFeedback, EffectResult::FeedbackApplied { had_feedback: true }) => {
                Some(Effect::BeginIteration)
            }
            (Effect::ApplyFeedback, EffectResult::FeedbackApplied { had_feedback: false }) => {
                Some(Effect::EvaluateFinishPolicies)
            }
            (
                Effect::EvaluateFinishPolicies,
                EffectResult::FinishPolicies { has_feedback: true },
            ) => Some(Effect::ApplyFeedback),
            (
                Effect::EvaluateFinishPolicies,
                EffectResult::FinishPolicies { has_feedback: false },
            ) => Some(Effect::PersistTurn),
            (Effect::PersistTurn, EffectResult::Done) => Some(Effect::FinishTurn),
            (Effect::FinishTurn, EffectResult::Done) => None,
            (effect, result) => {
                return Err(crate::AgentError::Config(format!(
                    "invalid turn effect result: {result:?} for {effect:?}"
                )));
            }
        };
        if let Some(next) = next {
            self.transition(next);
        }
        Ok(next)
    }
    fn queue_feedback(&mut self, feedback: impl IntoIterator<Item = String>) {
        self.feedback.extend(feedback);
    }
    fn take_feedback(&mut self) -> Vec<String> {
        std::mem::take(&mut self.feedback)
    }
    fn iteration(&self) -> usize {
        self.iteration
    }
    fn begin_iteration(&mut self, max: Option<usize>) -> Result<usize, crate::AgentError> {
        if let Some(max) = max
            && self.iteration >= max
        {
            return Err(crate::AgentError::MaxIterations(max));
        }
        self.iteration += 1;
        Ok(self.iteration)
    }
    fn request_thinking(&mut self) {
        self.force_thinking = true;
    }
    fn model_hint(&mut self, config: &crate::AgentConfig) -> crate::ModelHint {
        if self.force_thinking {
            self.force_thinking = false;
            return crate::ModelHint::Thinking;
        }
        if config.path == crate::TaskPath::Fast {
            crate::ModelHint::Execution
        } else if self.previous_tool_error {
            crate::ModelHint::Recovery
        } else if self.consecutive_noop >= 3 || self.iteration == 1 {
            crate::ModelHint::Thinking
        } else if config.path == crate::TaskPath::Heavy && self.iteration.is_multiple_of(4) {
            crate::ModelHint::Thinking
        } else {
            crate::ModelHint::Execution
        }
    }
    fn observe_assistant(&mut self, message: &crate::Message) {
        let calls = message.tool_calls().next().is_some();
        if calls && message.text_content().is_empty() {
            self.consecutive_noop += 1;
        } else if calls {
            self.consecutive_noop = 0;
        }
    }
    fn observe_tool_results(&mut self, message: &crate::Message) {
        self.previous_tool_error = message.tool_results_iter().any(|result| result.is_error);
    }
}

const MAX_CONSECUTIVE_COMPACTION_FAILURES: usize = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_owns_effect_transitions_and_feedback() {
        let mut machine = TurnMachine::new(1);
        assert_eq!(machine.effect(), Effect::BeginTurn);
        machine.queue_feedback(["review".to_string()]);
        machine.transition(Effect::ApplyFeedback);
        assert_eq!(machine.effect(), Effect::ApplyFeedback);
        assert_eq!(machine.take_feedback(), ["review"]);
        assert!(machine.advance(EffectResult::FeedbackApplied { had_feedback: true }).is_ok());
    }
}
