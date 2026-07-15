use std::sync::Arc;

use crate::context::ContextOps;
use crate::error::AgentError;
use crate::hooks::{HookContext, HookPhase};
use crate::message::Message;
use crate::session::SessionOps;
use crate::turn::TurnEvent;

pub(super) async fn run_hook_phase<S, C>(
    session: &mut S,
    context: &mut C,
    phase: HookPhase,
    hook_context: &HookContext,
    assistant_message: &Message,
) -> Result<Vec<TurnEvent>, AgentError>
where
    S: SessionOps,
    C: ContextOps,
{
    let mut events = Vec::new();
    let phase_name = phase.name().to_string();
    let hooks = session.config().hooks.hooks_for_phase(&phase);
    for hook in hooks {
        events.push(TurnEvent::HookStarted {
            phase: phase_name.clone(),
            name: hook.name().to_string(),
        });
        let maybe_message = hook.run(hook_context, assistant_message).await?;
        let emitted = maybe_message.is_some();
        if let Some(message) = maybe_message {
            context.push_message(message.clone());
            events.push(TurnEvent::Assistant(message));
        }
        if emitted {
            context.push_system_reminder(crate::message::SystemReminder::HookInterception {
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
    Arc::make_mut(&mut session.config_mut().hooks).remove_once_hooks();
    Ok(events)
}
