use std::sync::Arc;

use crate::agent::context::Conversation;
use crate::agent::hooks::{HookContext, HookPhase};
use crate::agent::turn::TurnEvent;
use crate::error::AgentError;
use crate::model::message::Message;

use super::super::session::SessionInfo;

pub(super) async fn run_hook_phase(
    session: &mut SessionInfo,
    context: &mut Conversation,
    phase: HookPhase,
    hook_context: &HookContext,
    assistant_message: &Message,
) -> Result<Vec<TurnEvent>, AgentError> {
    let mut events = Vec::new();
    let phase_name = phase.name().to_string();
    let hooks = session.config().hooks.hooks_for_phase(&phase);
    for hook in hooks {
        let started =
            TurnEvent::HookStarted { phase: phase_name.clone(), name: hook.name().to_string() };
        session.emit_turn_event(&started);
        events.push(started);
        let maybe_message = hook.run(hook_context, assistant_message).await?;
        let emitted = maybe_message.is_some();
        if let Some(message) = maybe_message {
            context.push_message(message.clone());
            let event = TurnEvent::Assistant(message);
            session.emit_turn_event(&event);
            events.push(event);
        }
        let completed = TurnEvent::HookCompleted {
            phase: phase_name.clone(),
            name: hook.name().to_string(),
            emitted_message: emitted,
        };
        session.emit_turn_event(&completed);
        events.push(completed);
    }
    Arc::make_mut(&mut session.config_mut().hooks).remove_once_hooks();
    Ok(events)
}
