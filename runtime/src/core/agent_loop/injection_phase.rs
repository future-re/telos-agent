use crate::context::ContextOps;
use crate::session::SessionOps;
use crate::prompt::PromptProfile;
use tracing::debug;

pub(super) fn inject_memory<S: SessionOps, C: ContextOps>(
    session: &mut S,
    context: &mut C,
    user_input: &str,
    turn_id: u64,
    iterations: usize,
) {
    if iterations != 1 {
        return;
    }
    let Some(injector) = &session.config().memory_injector else { return };
    let Some(injection) = injector.inject_for_query(user_input) else { return };

    let unchanged = context.last_memory_fingerprint() == Some(injection.fingerprint);
    if context.memory_dirty() || !unchanged {
        debug!(
            session_id = %session.session_id(),
            turn_id,
            fingerprint = injection.fingerprint,
            memory_state_dirty = context.memory_dirty(),
            "injecting memory reminder"
        );
        context.push_system_reminder(injection.reminder);
        context.set_last_memory_fingerprint(Some(injection.fingerprint));
        context.set_turn_memory_injected(true);
    } else {
        debug!(
            session_id = %session.session_id(),
            turn_id,
            fingerprint = injection.fingerprint,
            "skipping unchanged memory reminder"
        );
    }
    context.set_memory_dirty(false);
}

pub(super) fn inject_skill<S: SessionOps, C: ContextOps>(
    session: &mut S,
    context: &mut C,
    user_input: &str,
    turn_id: u64,
    iterations: usize,
) {
    if iterations != 1 {
        return;
    }
    if session.config().prompt_profile != PromptProfile::Minimal {
        return;
    }
    let Some(injector) = &session.config().skill_injector else { return };
    let Some(injection) = injector.inject_for_query(user_input) else { return };

    let unchanged = context.last_skill_fingerprint() == Some(injection.fingerprint);
    if !unchanged {
        debug!(
            session_id = %session.session_id(),
            turn_id,
            fingerprint = injection.fingerprint,
            "injecting skill discovery reminder"
        );
        context.push_system_reminder(injection.reminder);
        context.set_last_skill_fingerprint(Some(injection.fingerprint));
    } else {
        debug!(
            session_id = %session.session_id(),
            turn_id,
            fingerprint = injection.fingerprint,
            "skipping unchanged skill discovery reminder"
        );
    }
}
