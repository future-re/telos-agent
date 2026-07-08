use std::sync::Arc;

use serde_json::Value;

use crate::message::{ContentBlock, Message, ThinkingBlock, ToolResult};

// ── Compression strategy ─────────────────────────────────────────────────────

/// Pluggable strategy for compressing a single content string.
pub trait ContentCompressor: Send + Sync {
    /// Compress `content` to fit within `target_bytes` (JSON-serialised byte length).
    fn compress(&self, content: &str, target_bytes: usize) -> String;
}

/// Brute-force Unicode-truncation compressor.
#[derive(Default)]
pub struct TruncationCompressor;

impl ContentCompressor for TruncationCompressor {
    fn compress(&self, input: &str, target_bytes: usize) -> String {
        if input.len() <= target_bytes {
            return input.to_string();
        }
        let mut lo = 0usize;
        let mut hi = input.chars().count();
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            let candidate: String = input.chars().take(mid).collect();
            if serde_json::to_string(&candidate).unwrap().len() <= target_bytes {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        input.chars().take(lo).collect()
    }
}

// ── Config ───────────────────────────────────────────────────────────────────

/// Knobs for per-message compaction. `None` disables the corresponding budget.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Maximum serialised bytes for a single content field (text / tool result / thinking).
    pub max_block_content_chars: Option<usize>,
    /// Maximum serialised bytes for the entire message.
    pub max_message_chars: Option<usize>,
    /// Compression strategy — defaults to [`TruncationCompressor`] when `None`.
    pub compressor: Option<Arc<dyn ContentCompressor>>,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self { max_block_content_chars: None, max_message_chars: None, compressor: None }
    }
}

// ── Result ───────────────────────────────────────────────────────────────────

/// Outcome of [`compact_message`].
#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub message: Message,
    pub compacted: bool,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Truncate content fields inside message blocks so the total serialised size
/// fits within the budget.
pub fn compact_message(
    message: Message,
    config: &CompactionConfig,
) -> CompactionResult {
    if config.max_block_content_chars.is_none() && config.max_message_chars.is_none() {
        return CompactionResult { message, compacted: false };
    }

    let compressor: &dyn ContentCompressor =
        config.compressor.as_deref().unwrap_or(&TruncationCompressor);

    // ── Measure content-lengths by serialising once ──────────────────────
    let serialized = serde_json::to_string(&message).unwrap();

    struct Entry { idx: usize, orig: usize, target: usize }
    let mut entries: Vec<Entry> = Vec::new();
    let mut orig_sum: usize = 0;

    for (i, block) in message.blocks.iter().enumerate() {
        if let Some(bytes) = content_serialized_len(block) {
            orig_sum += bytes;
            entries.push(Entry { idx: i, orig: bytes, target: bytes });
        }
    }

    if entries.is_empty() || orig_sum == 0 {
        return CompactionResult { message, compacted: false };
    }

    // ── Early exit: aggregate already fits (only need per-block cap) ────
    let total_len = serialized.len();
    if total_len <= config.max_message_chars.unwrap_or(usize::MAX) {
        if let Some(cap) = config.max_block_content_chars {
            return cap_only(message, entries, cap, compressor);
        }
        return CompactionResult { message, compacted: false };
    }

    let overhead = total_len.saturating_sub(orig_sum);

    // ── Step 1: per-block cap ───────────────────────────────────────────
    let mut compacted = false;
    if let Some(cap) = config.max_block_content_chars {
        for e in &mut entries {
            if e.target > cap {
                e.target = cap;
                compacted = true;
            }
        }
    }

    // ── Step 2: aggregate budget (proportional scaling) ─────────────────
    let content_sum: usize = entries.iter().map(|e| e.target).sum();
    let target_content_sum = config
        .max_message_chars
        .map(|budget| budget.saturating_sub(overhead))
        .unwrap_or(content_sum);

    if target_content_sum < content_sum {
        compacted = true;
        for e in &mut entries {
            let scaled = e.target * target_content_sum / content_sum;
            if scaled < e.target {
                e.target = scaled;
            }
        }
    }

    if !compacted {
        return CompactionResult { message, compacted: false };
    }

    // ── Apply truncation ────────────────────────────────────────────────
    let mut blocks = message.blocks;
    for e in &entries {
        if e.target < e.orig {
            truncate_block_content(&mut blocks[e.idx], e.target, compressor);
        }
    }
    CompactionResult { message: Message { role: message.role, blocks }, compacted: true }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Only per-block cap (aggregate budget wasn't exceeded).
fn cap_only(
    message: Message,
    mut entries: Vec<Entry>,
    cap: usize,
    compressor: &dyn ContentCompressor,
) -> CompactionResult {
    let mut compacted = false;
    for e in &mut entries {
        if e.orig > cap {
            e.target = cap;
            compacted = true;
        }
    }
    if !compacted {
        return CompactionResult { message, compacted: false };
    }
    let mut blocks = message.blocks;
    for e in &entries {
        if e.target < e.orig {
            truncate_block_content(&mut blocks[e.idx], e.target, compressor);
        }
    }
    CompactionResult { message: Message { role: message.role, blocks }, compacted: true }
}

/// Serialised byte length of the truncatable content inside a block.
fn content_serialized_len(block: &ContentBlock) -> Option<usize> {
    match block {
        ContentBlock::Text(t) => Some(serde_json::to_string(&t.text).unwrap().len()),
        ContentBlock::ToolResult(r) => Some(r.content.to_string().len()),
        ContentBlock::Thinking(t) => Some(serde_json::to_string(&t.text).unwrap().len()),
        ContentBlock::ToolCall(_) => None,
    }
}

/// Truncate the block's content field to approximately `target_bytes`.
fn truncate_block_content(
    block: &mut ContentBlock,
    target_bytes: usize,
    compressor: &dyn ContentCompressor,
) {
    match block {
        ContentBlock::Text(t) => {
            let compressed = compressor.compress(&t.text, target_bytes);
            t.text = format!("{}…[truncated]", compressed);
        }
        ContentBlock::ToolResult(r) => {
            let s = r.content.to_string();
            let compressed = compressor.compress(&s, target_bytes);
            r.content = Value::String(format!("{}…[truncated]", compressed));
        }
        ContentBlock::Thinking(t) => {
            let compressed = compressor.compress(&t.text, target_bytes);
            t.text = format!("{}…[truncated]", compressed);
        }
        ContentBlock::ToolCall(_) => {}
    }
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

    fn config(block_cap: Option<usize>, msg_cap: Option<usize>) -> CompactionConfig {
        CompactionConfig {
            max_block_content_chars: block_cap,
            max_message_chars: msg_cap,
            compressor: None,
        }
    }

    #[test]
    fn no_op_when_budget_disabled() {
        let msg = Message::tool_results(vec![tool_result(&"x".repeat(10_000))]);
        let r = compact_message(msg.clone(), &CompactionConfig::default());
        assert!(!r.compacted);
        assert_eq!(r.message, msg);
    }

    #[test]
    fn per_block_cap_truncates_oversized_tool_result() {
        let msg = Message::tool_results(vec![tool_result(&"x".repeat(10_000))]);
        let r = compact_message(msg, &config(Some(100), None));
        assert!(r.compacted);
    }

    #[test]
    fn aggregate_budget_truncates_multiple_blocks() {
        let msg = Message::tool_results(vec![
            tool_result(&"x".repeat(500)),
            tool_result(&"y".repeat(500)),
        ]);
        let r = compact_message(msg, &config(None, Some(300)));
        assert!(r.compacted);
        let s = serde_json::to_string(&r.message).unwrap();
        assert!(s.len() < 600, "len={}", s.len());
    }

    #[test]
    fn both_budgets_cap_then_scale() {
        let msg = Message::tool_results(vec![
            tool_result(&"x".repeat(10_000)),
            tool_result(&"y".repeat(10_000)),
        ]);
        let r = compact_message(msg, &config(Some(200), Some(300)));
        assert!(r.compacted);
        let s = serde_json::to_string(&r.message).unwrap();
        assert!(s.len() < 500, "len={}", s.len());
    }

    #[test]
    fn leafs_tool_calls_untouched() {
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
        let r = compact_message(msg, &config(Some(50), None));
        assert!(r.compacted);
        assert_eq!(r.message.tool_calls().count(), 1);
    }

    #[test]
    fn truncated_content_has_marker() {
        let msg = Message::user("hello world this is a long message".repeat(10));
        let r = compact_message(msg, &config(Some(30), None));
        assert!(r.compacted);
        assert!(r.message.text_content().contains("[truncated]"));
    }
}
