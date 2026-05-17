//! Model provider abstraction — pluggable LLM backends.
//!
//! Built-in backends: [`AnthropicProvider`], [`OpenAIProvider`].
//! The default [`ModelProvider::stream_complete`] wraps [`ModelProvider::complete`]
//! so non-streaming providers automatically get a (single-chunk) streaming impl.

use async_stream::try_stream;
use async_trait::async_trait;
use futures_core::stream::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;
use crate::message::{Message, ToolCall};
use crate::tool::ToolDefinition;

pub mod anthropic;
pub mod openai;

pub use anthropic::{AnthropicConfig, AnthropicProvider};
pub use openai::{OpenAIConfig, OpenAIProvider};

/// All inputs a provider needs to generate a single completion.
///
/// `system_prompt` is separate from `messages` because both Anthropic and
/// OpenAI accept the system prompt as a top-level field rather than a message.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub system_prompt: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
}

/// Parse an HTTP response body as a server-sent-events stream of `data:` payloads.
///
/// Buffers the byte stream until it sees an event separator (`\n\n` or
/// `\r\n\r\n`), then yields the concatenated `data:` lines as a single string.
/// `[DONE]` and empty events are filtered out so callers see only model data.
pub(crate) fn sse_data_stream(
    response: reqwest::Response,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<String, AgentError>> + Send>> {
    Box::pin(try_stream! {
        let mut buffer = String::new();
        let mut bytes = response.bytes_stream();
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|err| AgentError::Provider(err.to_string()))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // SSE events are separated by a blank line; tolerate both \n\n and \r\n\r\n.
            while let Some(split_at) = buffer.find("\n\n").or_else(|| buffer.find("\r\n\r\n")) {
                let raw = buffer[..split_at].to_string();
                let drain_to = if buffer[split_at..].starts_with("\r\n\r\n") {
                    split_at + 4
                } else {
                    split_at + 2
                };
                buffer.drain(..drain_to);

                // An event may carry multiple `data:` lines — join them with `\n` per spec.
                let data = raw
                    .lines()
                    .filter_map(|line| {
                        let line = line.trim_end_matches('\r');
                        line.strip_prefix("data:").map(|data| data.trim_start())
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                yield data;
            }
        }
    })
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
    /// Fully-assembled tool call from the assistant (providers buffer streamed JSON internally).
    ToolCall(ToolCall),
    /// Final marker carrying the stop reason and (optional) usage.
    MessageStop {
        stop_reason: StopReason,
        usage: Option<TokenUsage>,
    },
}

/// Abstract LLM backend. Implement this to plug in a new model provider.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Issue a single non-streaming completion request.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError>;

    /// Stream a completion as a sequence of [`ProviderEvent`]s.
    ///
    /// The default implementation calls [`complete`](Self::complete) and
    /// re-emits the result as one synthetic stream — providers that genuinely
    /// stream should override this for incremental delivery.
    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        Box::pin(try_stream! {
            let response = self.complete(request).await?;
            yield ProviderEvent::MessageStart;
            for block in &response.message.blocks {
                match block {
                    crate::message::ContentBlock::Text(text) => {
                        yield ProviderEvent::TextDelta(text.text.clone());
                    }
                    crate::message::ContentBlock::ToolCall(call) => {
                        yield ProviderEvent::ToolCall(call.clone());
                    }
                    crate::message::ContentBlock::ToolResult(_) => {}
                }
            }
            yield ProviderEvent::MessageStop {
                stop_reason: response.stop_reason,
                usage: response.usage,
            };
        })
    }

    /// Estimate the number of tokens for the given text.
    /// This is a local approximation used for budget checks before calling the API.
    fn estimate_tokens(&self, text: &str) -> usize;
}
