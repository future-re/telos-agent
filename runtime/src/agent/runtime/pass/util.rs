use crate::agent::turn::{TurnEvent, TurnInputReceiver};
use crate::model::message::Message;

use super::super::session::SessionInfo;

pub(super) fn drain_turn_input(turn_input: &mut TurnInputReceiver) -> Vec<Message> {
    let mut messages = Vec::new();
    while let Ok(input) = turn_input.try_recv() {
        let input = input.trim();
        if !input.is_empty() {
            messages.push(Message::user(input));
        }
    }
    messages
}

pub(super) fn drain_external_events(session: &mut SessionInfo) -> Vec<Message> {
    session
        .event_channel_mut()
        .as_mut()
        .map(|channel| {
            channel
                .try_drain_incoming()
                .iter()
                .map(crate::integrations::event_channel::EventChannel::to_system_message)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn broadcast(session: &SessionInfo, event: &TurnEvent) {
    session.emit_turn_event(event);
}
