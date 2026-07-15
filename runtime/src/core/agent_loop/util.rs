use crate::config::{AgentConfig, TaskPath};
use crate::message::Message;
use crate::provider::ModelHint;
use crate::session::SessionOps;
use crate::turn::{TurnEvent, TurnInputReceiver};

pub(super) fn resolve_hint(
    config: &AgentConfig,
    iteration: usize,
    previous_tool_error: bool,
    consecutive_noop: usize,
) -> ModelHint {
    let (hint, reason) = if config.path == TaskPath::Fast {
        (ModelHint::Execution, "fast path")
    } else if previous_tool_error {
        (ModelHint::Recovery, "tool error")
    } else if consecutive_noop >= 3 {
        (ModelHint::Thinking, "stuck detection")
    } else if iteration == 1 {
        (ModelHint::Thinking, "first iteration")
    } else if config.path == TaskPath::Heavy && iteration.is_multiple_of(4) {
        (ModelHint::Thinking, "heavy periodic rethink")
    } else {
        (ModelHint::Execution, "default")
    };

    tracing::debug!(
        iteration = iteration,
        hint = ?hint,
        reason = reason,
        path = ?config.path,
        previous_tool_error = previous_tool_error,
        consecutive_noop = consecutive_noop,
        "hint resolved"
    );

    hint
}

pub(super) fn drain_turn_input(turn_input: &mut TurnInputReceiver) -> Vec<Message> {
    let mut messages = Vec::new();
    while let Ok(input) = turn_input.try_recv() {
        let input = input.trim().to_string();
        if !input.is_empty() {
            messages.push(Message::user(input));
        }
    }
    messages
}

pub(super) fn drain_external_events<S: SessionOps>(session: &mut S) -> Vec<Message> {
    if let Some(ec) = session.event_channel_mut() {
        ec.try_drain_incoming()
            .iter()
            .map(|e| crate::event_channel::EventChannel::to_system_message(e))
            .collect()
    } else {
        Vec::new()
    }
}

pub(super) fn broadcast<S: SessionOps>(session: &S, event: &TurnEvent) {
    if let Some(ec) = session.event_channel() {
        ec.publish(event);
    }
}
