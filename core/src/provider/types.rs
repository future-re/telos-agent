//! Request/response types shared across all providers.

use serde::{Deserialize, Serialize};

use crate::message::{Message, ToolCall};
use crate::prompt::PromptBlock;
use crate::tool::ToolDefinition;

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
}

impl TokenUsage {
    /// Sum of input and output tokens.
    pub fn total(&self) -> usize {
        self.input_tokens + self.output_tokens
    }
}

/// Aggregated result of a non-streaming completion.
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub message: Message,
    pub stop_reason: StopReason,
    pub usage: Option<TokenUsage>,
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
    MessageStop { stop_reason: StopReason, usage: Option<TokenUsage> },
}
