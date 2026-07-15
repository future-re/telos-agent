//! Runtime input channel for interactive hosts.

use tokio::sync::mpsc;

/// Sender used by hosts to submit user input into an active turn.
pub type TurnInputSender = mpsc::UnboundedSender<String>;

/// Receiver consumed by the runtime while a turn is active.
pub type TurnInputReceiver = mpsc::UnboundedReceiver<String>;

/// Build a channel for live user input during a turn.
pub fn turn_input_channel() -> (TurnInputSender, TurnInputReceiver) {
    mpsc::unbounded_channel()
}

pub(super) fn empty_turn_input_receiver() -> TurnInputReceiver {
    let (_tx, rx) = turn_input_channel();
    rx
}
