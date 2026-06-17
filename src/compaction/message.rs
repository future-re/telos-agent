//! Message-level compaction: truncates individual tool results that exceed a char limit.
//!
//! Tool outputs (e.g. `cat` on a large file) can drown the context window even
//! when the history is small. This module replaces oversized [`ToolResult`]
//! payloads with a `{ preview, truncated, original_char_count }` JSON object so
//! the model still sees something useful while staying under budget.

use serde_json::json;

use crate::message::{Message, ToolResult};

/// Knobs for per-message compaction.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Max characters in a single [`ToolResult::content`] serialisation. `usize::MAX` disables truncation.
    pub max_tool_result_chars: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self { max_tool_result_chars: usize::MAX }
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
    // Fast path: budget disabled.
    if config.max_tool_result_chars == usize::MAX {
        return CompactionResult { message, compacted: false };
    }

    let mut changed = false;
    let mut results = Vec::new();
    for result in message.tool_results_iter() {
        let content = result.content.to_string();
        if content.len() > config.max_tool_result_chars {
            changed = true;
            // Replace the payload with a structured preview; the original size
            // is preserved so the model knows how much was dropped.
            results.push(ToolResult {
                tool_call_id: result.tool_call_id.clone(),
                name: result.name.clone(),
                is_error: result.is_error,
                content: json!({
                    "preview": truncate_chars(&content, config.max_tool_result_chars),
                    "truncated": true,
                    "original_char_count": content.len(),
                }),
            });
        } else {
            results.push(result.clone());
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
            &CompactionConfig::default(), // usize::MAX
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
            &CompactionConfig { max_tool_result_chars: 100 },
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
            &CompactionConfig { max_tool_result_chars: 100 },
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
            &CompactionConfig { max_tool_result_chars: 50 },
        );
        assert!(result.compacted);
        // The compacted content should have the preview and truncated marker.
        let compacted = result.message.tool_results_iter().next().unwrap();
        let content = &compacted.content;
        assert_eq!(content["truncated"], json!(true));
        assert_eq!(content["original_char_count"], json!(211)); // {"text":" + 200 x's + "}
        // Preview is at most max_tool_result_chars chars.
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
            &CompactionConfig { max_tool_result_chars: 40 },
        );
        assert!(result.compacted);
        let compacted = result.message.tool_results_iter().next().unwrap();
        let preview = compacted.content["preview"].as_str().unwrap();
        assert!(preview.chars().count() <= 40);
    }
}
