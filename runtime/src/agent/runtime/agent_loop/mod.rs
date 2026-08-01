//! Internal agent turn loop.
//!
//! [`run_turn`] coordinates one or more model iterations. The surrounding
//! runtime owns sessions and concurrency; this module owns only turn execution.

mod compaction;
mod injection;
mod input;
mod provider;
mod state;
mod tools;
mod turn;

use crate::model::message::Message;
use crate::model::provider::StopReason;

pub(crate) use turn::run_turn;

/// Internal result of executing a turn. Public event history is assembled by
/// the runtime facade from the session's event log.
pub(crate) struct TurnOutcome {
    pub(crate) final_message: Message,
    pub(crate) stop_reason: StopReason,
}
