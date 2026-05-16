use serde_json::json;

use crate::message::{Message, ToolResult};

#[derive(Debug, Clone)]
pub struct CompactionConfig {
    pub max_tool_result_chars: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            max_tool_result_chars: usize::MAX,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub message: Message,
    pub compacted: bool,
}

pub fn compact_tool_result_message(
    message: Message,
    config: &CompactionConfig,
) -> CompactionResult {
    if config.max_tool_result_chars == usize::MAX {
        return CompactionResult {
            message,
            compacted: false,
        };
    }

    let mut changed = false;
    let mut results = Vec::new();
    for result in message.tool_results_iter() {
        let content = result.content.to_string();
        if content.len() > config.max_tool_result_chars {
            changed = true;
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
        return CompactionResult {
            message,
            compacted: false,
        };
    }

    CompactionResult {
        message: Message::tool_results(results),
        compacted: true,
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}
