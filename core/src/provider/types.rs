//! Request/response types shared across all providers.

use serde::{Deserialize, Serialize};

use crate::message::{Message, ToolCall};
use crate::prompt::PromptBlock;
use crate::tool::ToolDefinition;

/// Semantic routing hint — describes the nature of a provider call so that
/// a routing provider can select an appropriate model.
///
/// Providers that don't support routing ignore this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelHint {
    /// Strategic reasoning: understanding user intent, planning, complex decisions.
    Thinking,
    /// Tool execution: processing tool results, simple file operations, retrieval.
    Execution,
    /// Error recovery: re-evaluating and re-planning after a tool failure.
    Recovery,
    /// Summarization: conversation compaction, history compression.
    Summarization,
}

/// All inputs a provider needs to generate a single completion.
///
/// `system_prompt` is separate from `messages` because OpenAI-compatible
/// providers accept the system prompt as a top-level field rather than a message.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub system_prompt: Option<String>,
    /// Optional structured system prompt blocks for providers that support
    /// per-block cache control (e.g., Anthropic prompt caching).
    pub system_prompt_blocks: Option<Vec<PromptBlock>>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    /// Optional model routing hint. When `None`, the provider uses its default model.
    /// When `Some`, a routing-aware provider may select a different model.
    pub model_hint: Option<ModelHint>,
}

/// Why the model stopped emitting tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    /// Model produced a final answer and the turn should end.
    EndTurn,
    /// Model requested one or more tool calls; the executor must run them.
    ToolUse,
}

/// Token accounting reported by the provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub total_tokens: Option<usize>,
    pub prompt_cache_hit_tokens: Option<usize>,
    pub prompt_cache_miss_tokens: Option<usize>,
    pub reasoning_tokens: Option<usize>,
}

impl TokenUsage {
    /// Build usage from the common input/output pair reported by all providers.
    pub fn new(input_tokens: usize, output_tokens: usize) -> Self {
        Self {
            input_tokens,
            output_tokens,
            total_tokens: None,
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            reasoning_tokens: None,
        }
    }

    /// Sum of input and output tokens.
    pub fn total(&self) -> usize {
        self.total_tokens.unwrap_or(self.input_tokens + self.output_tokens)
    }
}

/// Aggregated result of a non-streaming completion.
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub message: Message,
    pub stop_reason: StopReason,
    pub usage: Option<TokenUsage>,
    pub model: Option<String>,
}

/// One unit of incremental output from a streaming completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProviderEvent {
    /// Marker emitted before any content.
    MessageStart,
    /// Incremental text chunk to append to the in-flight assistant message.
    TextDelta(String),
    /// Incremental reasoning chunk from a thinking-capable model.
    ThinkingDelta(String),
    /// Fully-assembled tool call from the assistant (providers buffer streamed JSON internally).
    ToolCall(ToolCall),
    /// Final marker carrying the stop reason and (optional) usage.
    MessageStop { stop_reason: StopReason, usage: Option<TokenUsage>, model: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_hint_is_none_by_default() {
        let req = CompletionRequest {
            system_prompt: None,
            system_prompt_blocks: None,
            messages: vec![],
            tools: vec![],
            model_hint: None,
        };
        assert!(req.model_hint.is_none());
    }

    #[test]
    fn model_hint_can_be_set() {
        let req = CompletionRequest {
            system_prompt: None,
            system_prompt_blocks: None,
            messages: vec![],
            tools: vec![],
            model_hint: Some(ModelHint::Thinking),
        };
        assert_eq!(req.model_hint, Some(ModelHint::Thinking));
    }

    #[test]
    fn model_hint_is_copy_and_eq() {
        let a = ModelHint::Thinking;
        let b = a;
        assert_eq!(a, b);
    }
}
