use tokio::sync::mpsc;

pub type TurnInputSender = mpsc::UnboundedSender<String>;
pub type TurnInputReceiver = mpsc::UnboundedReceiver<String>;

pub fn turn_input_channel() -> (TurnInputSender, TurnInputReceiver) {
    mpsc::unbounded_channel()
}

pub fn empty_turn_input_receiver() -> TurnInputReceiver {
    let (_tx, rx) = turn_input_channel();
    rx
}
