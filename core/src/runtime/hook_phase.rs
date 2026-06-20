use std::sync::Arc;

use crate::error::AgentError;
use crate::hooks::{HookContext, HookPhase};
use crate::message::Message;
use crate::runtime::{AgentSession, TurnEvent};

impl AgentSession {
    /// Run all hooks registered for a given phase and append any emitted messages.
    pub(super) async fn run_hook_phase(
        &mut self,
        phase: HookPhase,
        hook_context: &HookContext,
        assistant_message: &Message,
    ) -> Result<Vec<TurnEvent>, AgentError> {
        let mut events = Vec::new();
        let phase_name = phase.name().to_string();
        let hooks = self.config.hooks.hooks_for_phase(&phase);
        for hook in hooks {
            events.push(TurnEvent::HookStarted {
                phase: phase_name.clone(),
                name: hook.name().to_string(),
            });
            let maybe_message = hook.run(hook_context, assistant_message).await?;
            let emitted = maybe_message.is_some();
            if let Some(message) = maybe_message {
                self.messages.push(message.clone());
                events.push(TurnEvent::Assistant(message));
            }
            if emitted {
                self.push_system_reminder(crate::message::SystemReminder::HookInterception {
                    phase: phase_name.clone(),
                    name: hook.name().to_string(),
                });
            }
            events.push(TurnEvent::HookCompleted {
                phase: phase_name.clone(),
                name: hook.name().to_string(),
                emitted_message: emitted,
            });
        }
        // Clean up one-shot hooks after each phase execution.
        Arc::make_mut(&mut self.config.hooks).remove_once_hooks();
        Ok(events)
    }
}
