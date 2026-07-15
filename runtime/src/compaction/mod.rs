//! Conversation compaction — keep the working context small enough to fit.
//!
//! Two levels of compaction are provided:
//! - [`message_truncation`](crate::compaction::message_truncation) — per-message truncation.
//! - [`history_summary`](crate::compaction::history_summary) — history-level summarisation.
//!
//! The runtime applies both during the turn loop; see [`AgentSession::run_turn_stream`](crate::AgentSession::run_turn_stream).

pub mod history_summary;
pub mod message_truncation;

use crate::message::{ContentBlock, Message};
use crate::provider::ModelProvider;

pub use history_summary::{HistoryCompactionStrategy, SummaryHistoryCompaction};
pub use message_truncation::{
    ContentCompressor, MessageTruncationConfig, MessageTruncationResult, TruncationCompressor,
    truncate_message,
};

/// Sum estimated token counts across every block in `messages`.
pub(crate) fn estimate_message_tokens(messages: &[Message], provider: &dyn ModelProvider) -> usize {
    messages
        .iter()
        .flat_map(|message| message.blocks.iter())
        .map(|block| match block {
            ContentBlock::Text(text) => provider.estimate_tokens(&text.text),
            ContentBlock::Thinking(thinking) => provider.estimate_tokens(&thinking.text),
            ContentBlock::ToolCall(call) => {
                provider.estimate_tokens(&call.name)
                    + provider.estimate_tokens(&call.arguments.to_string())
            }
            ContentBlock::ToolResult(result) => {
                provider.estimate_tokens(&result.content.to_string())
            }
        })
        .sum()
}
