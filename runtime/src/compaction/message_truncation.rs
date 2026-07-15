//! Message-level truncation: cap oversized text, thinking, and tool-result fields.
//!
//! This layer shrinks individual messages without changing conversation shape.

use std::{fmt::Debug, sync::Arc};

use serde_json::Value;

use crate::message::{ContentBlock, Message, Role};

// ── Compression strategy ─────────────────────────────────────────────────────

/// Pluggable strategy for compressing a single content string.
pub trait ContentCompressor: Debug + Send + Sync {
    /// Compress `content` so its JSON-serialised form fits within `target_bytes`.
    fn compress(&self, content: &str, target_bytes: usize) -> String;
}

/// Brute-force Unicode truncation — keeps the longest prefix whose serialised
/// JSON form fits in `target_bytes`.
#[derive(Debug, Default)]
pub struct TruncationCompressor;

impl ContentCompressor for TruncationCompressor {
    fn compress(&self, input: &str, target_bytes: usize) -> String {
        if serde_json::to_string(input).unwrap().len() <= target_bytes {
            return input.to_string();
        }

        let lo = binary_search_largest_fitting(input.chars().count(), |mid| {
            let candidate: String = input.chars().take(mid).collect();
            serde_json::to_string(&candidate).unwrap().len() <= target_bytes
        });
        input.chars().take(lo).collect()
    }
}

// ── Config ───────────────────────────────────────────────────────────────────

/// Knobs for message-level truncation. `None` disables the corresponding budget.
#[derive(Debug, Clone, Default)]
pub struct MessageTruncationConfig {
    /// Maximum serialised bytes for a single content field (text / tool result / thinking).
    pub max_block_content_bytes: Option<usize>,
    /// Maximum serialised bytes for the entire message.
    pub max_message_bytes: Option<usize>,
    /// Compression strategy — defaults to [`TruncationCompressor`] when `None`.
    pub compressor: Option<Arc<dyn ContentCompressor>>,
}

// ── Result ───────────────────────────────────────────────────────────────────

/// Outcome of [`truncate_message`].
#[derive(Debug, Clone)]
pub struct MessageTruncationResult {
    pub message: Message,
    pub compacted: bool,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Compress content fields inside message blocks so the total serialised size
/// fits within the budget. Delegates to the configured [`ContentCompressor`].
pub fn truncate_message(
    message: Message,
    config: &MessageTruncationConfig,
) -> MessageTruncationResult {
    if config.max_block_content_bytes.is_none() && config.max_message_bytes.is_none() {
        return MessageTruncationResult { message, compacted: false };
    }

    let compressor: &dyn ContentCompressor =
        config.compressor.as_deref().unwrap_or(&TruncationCompressor);
    let Message { role, mut blocks } = message;
    let mut compacted = false;

    if let Some(max_block_content_bytes) = config.max_block_content_bytes {
        compacted |= compress_blocks_over_cap(&mut blocks, max_block_content_bytes, compressor);
    }

    if let Some(max_message_bytes) = config.max_message_bytes
        && message_serialized_len(role, &blocks) > max_message_bytes
    {
        let cap = largest_block_cap_for_message(role, &blocks, max_message_bytes, compressor);
        compacted |= compress_blocks_over_cap(&mut blocks, cap, compressor);
    }

    MessageTruncationResult { message: Message { role, blocks }, compacted }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Serialised byte length of the truncatable content inside a block.
pub(crate) fn content_serialized_len(block: &ContentBlock) -> Option<usize> {
    match block {
        ContentBlock::Text(t) => Some(serde_json::to_string(&t.text).unwrap().len()),
        ContentBlock::ToolResult(r) => Some(r.content.to_string().len()),
        ContentBlock::Thinking(t) => Some(serde_json::to_string(&t.text).unwrap().len()),
        ContentBlock::ToolCall(_) => None,
    }
}

fn largest_block_cap_for_message(
    role: Role,
    blocks: &[ContentBlock],
    max_message_bytes: usize,
    compressor: &dyn ContentCompressor,
) -> usize {
    let upper_bound = blocks.iter().filter_map(content_serialized_len).max().unwrap_or(0);

    binary_search_largest_fitting(upper_bound, |cap| {
        let mut capped_blocks = blocks.to_vec();
        compress_blocks_over_cap(&mut capped_blocks, cap, compressor);
        message_serialized_len(role, &capped_blocks) <= max_message_bytes
    })
}

/// Return the largest value in `0..=upper_bound` accepted by a monotonic predicate.
fn binary_search_largest_fitting(
    mut upper_bound: usize,
    mut fits: impl FnMut(usize) -> bool,
) -> usize {
    let mut lo = 0usize;

    while lo < upper_bound {
        let mid = (lo + upper_bound).div_ceil(2);
        if fits(mid) {
            lo = mid;
        } else {
            upper_bound = mid - 1;
        }
    }

    lo
}

fn message_serialized_len(role: Role, blocks: &[ContentBlock]) -> usize {
    serde_json::to_string(&Message { role, blocks: blocks.to_vec() }).unwrap().len()
}

fn compress_blocks_over_cap(
    blocks: &mut [ContentBlock],
    max_content_bytes: usize,
    compressor: &dyn ContentCompressor,
) -> bool {
    let mut compacted = false;

    for block in blocks {
        if content_serialized_len(block).is_some_and(|bytes| bytes > max_content_bytes) {
            truncate_block_content(block, max_content_bytes, compressor);
            compacted = true;
        }
    }

    compacted
}

const TRUNCATION_MARKER: &str = "...[truncated]";

/// Compress the block's content field to fit within `target_bytes`.
pub(crate) fn truncate_block_content(
    block: &mut ContentBlock,
    target_bytes: usize,
    compressor: &dyn ContentCompressor,
) {
    match block {
        ContentBlock::Text(t) => {
            t.text = compress_with_marker(&t.text, target_bytes, compressor);
        }
        ContentBlock::ToolResult(r) => {
            let s = r.content.to_string();
            r.content = Value::String(compress_with_marker(&s, target_bytes, compressor));
        }
        ContentBlock::Thinking(t) => {
            t.text = compress_with_marker(&t.text, target_bytes, compressor);
        }
        ContentBlock::ToolCall(_) => {}
    }
}

fn compress_with_marker(
    content: &str,
    target_bytes: usize,
    compressor: &dyn ContentCompressor,
) -> String {
    let marker_budget = serde_json::to_string(TRUNCATION_MARKER).unwrap().len() - 2;
    if target_bytes <= marker_budget + 2 {
        return compressor.compress(content, target_bytes);
    }

    let compressed = compressor.compress(content, target_bytes - marker_budget);
    format!("{compressed}{TRUNCATION_MARKER}")
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::*;

    fn tool_result(text: &str) -> ToolResult {
        ToolResult {
            tool_call_id: "1".into(),
            name: "t".into(),
            is_error: false,
            content: serde_json::json!({"data": text}),
        }
    }

    fn config(block_cap: Option<usize>, msg_cap: Option<usize>) -> MessageTruncationConfig {
        MessageTruncationConfig {
            max_block_content_bytes: block_cap,
            max_message_bytes: msg_cap,
            compressor: None,
        }
    }

    #[test]
    fn no_op_when_budget_disabled() {
        let msg = Message::tool_results(vec![tool_result(&"x".repeat(10_000))]);
        let r = truncate_message(msg.clone(), &MessageTruncationConfig::default());
        assert!(!r.compacted);
        assert_eq!(r.message, msg);
    }

    #[test]
    fn per_block_cap_truncates_oversized_tool_result() {
        let msg = Message::tool_results(vec![tool_result(&"x".repeat(10_000))]);
        let r = truncate_message(msg, &config(Some(100), None));
        assert!(r.compacted);
        let block_len = content_serialized_len(&r.message.blocks[0]).unwrap();
        assert!(block_len <= 100, "len={}", block_len);
    }

    #[test]
    fn default_compressor_respects_json_serialized_length() {
        let input = "\\".repeat(100);
        let compressed = TruncationCompressor.compress(&input, 20);
        let serialized_len = serde_json::to_string(&compressed).unwrap().len();
        assert!(serialized_len <= 20, "len={}", serialized_len);
    }

    #[test]
    fn aggregate_budget_truncates_multiple_blocks() {
        let msg = Message::tool_results(vec![
            tool_result(&"x".repeat(500)),
            tool_result(&"y".repeat(500)),
        ]);
        let r = truncate_message(msg, &config(None, Some(300)));
        assert!(r.compacted);
        let s = serde_json::to_string(&r.message).unwrap();
        assert!(s.len() < 600, "len={}", s.len());
    }

    #[test]
    fn both_budgets_fit_message_budget() {
        let msg = Message::tool_results(vec![
            tool_result(&"x".repeat(10_000)),
            tool_result(&"y".repeat(10_000)),
        ]);
        let r = truncate_message(msg, &config(Some(200), Some(300)));
        assert!(r.compacted);
        let s = serde_json::to_string(&r.message).unwrap();
        assert!(s.len() < 500, "len={}", s.len());
    }

    #[test]
    fn leaves_tool_calls_untouched() {
        let msg = Message {
            role: Role::Assistant,
            blocks: vec![
                ContentBlock::Text(TextBlock { text: "x".repeat(10_000) }),
                ContentBlock::ToolCall(ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    arguments: serde_json::json!({"path": "/etc/passwd"}),
                }),
            ],
        };
        let r = truncate_message(msg, &config(Some(50), None));
        assert!(r.compacted);
        assert_eq!(r.message.tool_calls().count(), 1);
    }

    #[test]
    fn truncated_content_has_marker() {
        let msg = Message::user("hello world this is a long message".repeat(10));
        let r = truncate_message(msg, &config(Some(30), None));
        assert!(r.compacted);
        assert!(r.message.text_content().contains("[truncated]"));
    }
}
