use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;
use crate::message::Message;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    EndTurn,
    ToolUse,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError>;

    /// Estimate the number of tokens for the given text.
    /// This is a local approximation used for budget checks before calling the API.
    fn estimate_tokens(&self, text: &str) -> usize;
}
