use crate::agent::turn::TurnInputReceiver;
use crate::model::message::Message;

use super::super::session::SessionInfo;

/// Drains user input and external events that arrived while a turn was running.
pub(super) fn drain_pending(
    session: &mut SessionInfo,
    turn_input: &mut TurnInputReceiver,
) -> Vec<Message> {
    let mut messages = Vec::new();
    while let Ok(input) = turn_input.try_recv() {
        let input = input.trim();
        if !input.is_empty() {
            messages.push(Message::user(input));
        }
    }

    if let Some(channel) = session.event_channel_mut() {
        messages.extend(
            channel
                .try_drain_incoming()
                .iter()
                .map(crate::integrations::event_channel::EventChannel::to_system_message),
        );
    }
    messages
}
