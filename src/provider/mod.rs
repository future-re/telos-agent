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

#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub message: Message,
    pub stop_reason: StopReason,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError>;
}
