//! Conversation compaction — keep the working context small enough to fit.
//!
//! Two levels of compaction are provided:
//! - [`message`](crate::compaction::message) — per-message truncation (e.g. cap individual tool result size).
//! - [`strategy`](crate::compaction::strategy) — history-level summarisation (collapse old turns into a summary block).
//!
//! The runtime applies both during the turn loop; see [`AgentSession::run_turn_stream`](crate::AgentSession::run_turn_stream).

pub mod message;
pub mod strategy;

pub use message::{
    CompactionConfig, CompactionResult, ContentCompressor, TruncationCompressor, compact_message,
};
pub use strategy::{CompactionStrategy, SummaryCompaction};
