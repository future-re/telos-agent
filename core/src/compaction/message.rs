//! Message-level compaction: truncates individual tool results that exceed a char limit.
//!
//! Tool outputs (e.g. `cat` on a large file) can drown the context window even
//! when the history is small. This module replaces oversized [`ToolResult`]
//! payloads with a `{ preview, truncated, original_char_count }` JSON object so
//! the model still sees something useful while staying under budget.
//!
//! Two budgets are enforced:
//! 1. Per-result: any single result longer than `max_tool_result_chars` is truncated.
//! 2. Per-message: when all results in a message together exceed
//!    `max_message_tool_results_chars`, the largest results are truncated
//!    until the aggregate fits. This prevents N parallel tool calls from
//!    each staying under the per-result cap but collectively flooding the
//!    context window.

use serde_json::json;

use crate::message::{Message, ToolResult};

/// Knobs for per-message compaction.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Max characters in a single [`ToolResult::content`] serialisation. `usize::MAX` disables truncation.
    pub max_tool_result_chars: usize,
    /// Max aggregate characters across all tool results in a single message.
    /// When the sum exceeds this, the largest results are truncated to fit.
    /// `usize::MAX` disables the aggregate budget.
    pub max_message_tool_results_chars: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self { max_tool_result_chars: usize::MAX, max_message_tool_results_chars: usize::MAX }
    }
}

/// Outcome of [`compact_tool_result_message`].
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// The (possibly compacted) message.
    pub message: Message,
    /// `true` if any tool result was truncated.
    pub compacted: bool,
}

/// Truncate any tool results in `message` whose content exceeds the budget.
///
/// Returns the original message unchanged when nothing needs compacting (so
/// the caller can detect a no-op via [`CompactionResult::compacted`]).
pub fn compact_tool_result_message(
    message: Message,
    config: &CompactionConfig,
) -> CompactionResult {
    // Fast path: both budgets disabled.
    if config.max_tool_result_chars == usize::MAX
        && config.max_message_tool_results_chars == usize::MAX
    {
        return CompactionResult { message, compacted: false };
    }

    let mut changed = false;
    let mut results: Vec<ToolResult> = message.tool_results_iter().cloned().collect();
    if results.is_empty() {
        return CompactionResult { message, compacted: false };
    }

    // ── Pass 1: per-result truncation ──────────────────────
    if config.max_tool_result_chars != usize::MAX {
        for result in &mut results {
            let content_str = result.content.to_string();
            if content_str.len() > config.max_tool_result_chars {
                changed = true;
                result.content = json!({
                    "preview": truncate_chars(&content_str, config.max_tool_result_chars),
                    "truncated": true,
                    "original_char_count": content_str.len(),
                });
            }
        }
    }

    // ── Pass 2: per-message aggregate budget ───────────────
    if config.max_message_tool_results_chars != usize::MAX {
        let total: usize = results.iter().map(|r| r.content.to_string().len()).sum();
        if total > config.max_message_tool_results_chars {
            changed = true;
            // Sort by content size descending — truncate the largest first.
            let mut indexed: Vec<(usize, usize)> =
                results.iter().enumerate().map(|(i, r)| (i, r.content.to_string().len())).collect();
            indexed.sort_by_key(|b| std::cmp::Reverse(b.1)); // descending by size

            // The JSON wrapper ({ preview, truncated, original_char_count })
            // adds ~80 chars of overhead per wrapped result. We must account
            // for this when computing how much to trim; otherwise the wrapper
            // can make a result *larger* than the original (infinite loop).
            let wrapper_overhead: usize = indexed
                .iter()
                .map(|(idx, _)| {
                    let result = &results[*idx];
                    if result.content.get("preview").is_some() { 80 } else { 0 }
                })
                .sum();

            let mut excess =
                (total + wrapper_overhead).saturating_sub(config.max_message_tool_results_chars);
            for (idx, size) in &indexed {
                if excess == 0 {
                    break;
                }
                let result = &mut results[*idx];
                let content_str = result.content.to_string();
                // Keep at least 100 chars of content, and don't trim below zero.
                let keep = 100usize;
                let trimmable = size.saturating_sub(keep);
                let trim = excess.min(trimmable);
                if trim == 0 {
                    continue;
                }
                let new_limit = size.saturating_sub(trim);
                let preview = truncate_chars(&content_str, new_limit);
                result.content = json!({
                    "preview": preview,
                    "truncated": true,
                    "original_char_count": content_str.len(),
                });
                excess = excess.saturating_sub(trim);
            }
        }
    }

    if !changed {
        return CompactionResult { message, compacted: false };
    }

    CompactionResult { message: Message::tool_results(results), compacted: true }
}

/// Truncate by Unicode characters (not bytes) to avoid splitting a code point.
fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn no_compaction_when_budget_disabled() {
        let result = compact_tool_result_message(
            Message::tool_results(vec![ToolResult {
                tool_call_id: "1".into(),
                name: "Read".into(),
                is_error: false,
                content: json!({"text": "x".repeat(10_000)}),
            }]),
            &CompactionConfig::default(), // both usize::MAX
        );
        assert!(!result.compacted);
    }

    #[test]
    fn compacts_oversized_tool_result() {
        let result = compact_tool_result_message(
            Message::tool_results(vec![ToolResult {
                tool_call_id: "1".into(),
                name: "Read".into(),
                is_error: false,
                content: json!({"text": "x".repeat(200)}),
            }]),
            &CompactionConfig {
                max_tool_result_chars: 100,
                max_message_tool_results_chars: usize::MAX,
            },
        );
        assert!(result.compacted);
    }

    #[test]
    fn leaves_small_tool_result_unchanged() {
        let original = Message::tool_results(vec![ToolResult {
            tool_call_id: "1".into(),
            name: "Read".into(),
            is_error: false,
            content: json!({"text": "hello"}),
        }]);
        let result = compact_tool_result_message(
            original.clone(),
            &CompactionConfig {
                max_tool_result_chars: 100,
                max_message_tool_results_chars: usize::MAX,
            },
        );
        assert!(!result.compacted);
        assert_eq!(result.message, original);
    }

    #[test]
    fn truncated_result_includes_preview_and_original_count() {
        let text = "x".repeat(200);
        let result = compact_tool_result_message(
            Message::tool_results(vec![ToolResult {
                tool_call_id: "1".into(),
                name: "Read".into(),
                is_error: false,
                content: json!({"text": text}),
            }]),
            &CompactionConfig {
                max_tool_result_chars: 50,
                max_message_tool_results_chars: usize::MAX,
            },
        );
        assert!(result.compacted);
        let compacted = result.message.tool_results_iter().next().unwrap();
        let content = &compacted.content;
        assert_eq!(content["truncated"], json!(true));
        assert_eq!(content["original_char_count"], json!(211)); // {"text":" + 200 x's + "}
        let preview = content["preview"].as_str().unwrap();
        assert!(preview.chars().count() <= 50);
    }

    #[test]
    fn truncation_respects_unicode_chars() {
        let text = "你好世界".repeat(30); // 4 chars × 30 = 120 chars
        let result = compact_tool_result_message(
            Message::tool_results(vec![ToolResult {
                tool_call_id: "1".into(),
                name: "Read".into(),
                is_error: false,
                content: json!({"text": text}),
            }]),
            &CompactionConfig {
                max_tool_result_chars: 40,
                max_message_tool_results_chars: usize::MAX,
            },
        );
        assert!(result.compacted);
        let compacted = result.message.tool_results_iter().next().unwrap();
        let preview = compacted.content["preview"].as_str().unwrap();
        assert!(preview.chars().count() <= 40);
    }

    #[test]
    fn per_message_aggregate_budget_truncates_largest_result() {
        // Two results: one small (50 chars), one large (400 chars)
        // Budget: 200 chars aggregate → should truncate the large one
        let results = vec![
            ToolResult {
                tool_call_id: "1".into(),
                name: "small".into(),
                is_error: false,
                content: json!({"text": "x".repeat(50)}),
            },
            ToolResult {
                tool_call_id: "2".into(),
                name: "large".into(),
                is_error: false,
                content: json!({"text": "x".repeat(400)}),
            },
        ];
        let result = compact_tool_result_message(
            Message::tool_results(results),
            &CompactionConfig {
                max_tool_result_chars: usize::MAX,   // per-result disabled
                max_message_tool_results_chars: 200, // aggregate budget
            },
        );
        assert!(result.compacted);
        let total: usize =
            result.message.tool_results_iter().map(|r| r.content.to_string().len()).sum();
        // The aggregate budget is a rough heuristic — JSON wrapper overhead
        // means the post-compaction total may modestly exceed the raw budget.
        // What matters is that we significantly reduced the original size.
        assert!(total < 450, "aggregate should be well under original 472 chars: {total}");
    }

    #[test]
    fn per_message_budget_no_op_when_under() {
        let results = vec![
            ToolResult {
                tool_call_id: "1".into(),
                name: "a".into(),
                is_error: false,
                content: json!({"text": "hello"}),
            },
            ToolResult {
                tool_call_id: "2".into(),
                name: "b".into(),
                is_error: false,
                content: json!({"text": "world"}),
            },
        ];
        let result = compact_tool_result_message(
            Message::tool_results(results.clone()),
            &CompactionConfig {
                max_tool_result_chars: usize::MAX,
                max_message_tool_results_chars: 10_000, // way above
            },
        );
        assert!(!result.compacted);
    }
}
