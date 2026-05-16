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

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub system_prompt: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
}

pub(crate) fn sse_data_stream(
    response: reqwest::Response,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<String, AgentError>> + Send>> {
    Box::pin(try_stream! {
        let mut buffer = String::new();
        let mut bytes = response.bytes_stream();
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|err| AgentError::Provider(err.to_string()))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(split_at) = buffer.find("\n\n").or_else(|| buffer.find("\r\n\r\n")) {
                let raw = buffer[..split_at].to_string();
                let drain_to = if buffer[split_at..].starts_with("\r\n\r\n") {
                    split_at + 4
                } else {
                    split_at + 2
                };
                buffer.drain(..drain_to);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    EndTurn,
    ToolUse,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

impl TokenUsage {
    pub fn total(&self) -> usize {
        self.input_tokens + self.output_tokens
    }
}

#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub message: Message,
    pub stop_reason: StopReason,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProviderEvent {
    MessageStart,
    TextDelta(String),
    ToolCall(ToolCall),
    MessageStop {
        stop_reason: StopReason,
        usage: Option<TokenUsage>,
    },
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError>;

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
