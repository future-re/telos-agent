use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::provider::TokenUsage;

/// Extra DeepSeek chat-completions options that are not part of the generic
/// [`ModelProvider`](crate::ModelProvider) trait.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeepSeekChatOptions {
    pub response_format: Option<DeepSeekResponseFormat>,
    /// Assistant prefix used by DeepSeek beta prefix completion.
    pub prefix: Option<String>,
    /// Optional stop sequences forwarded to chat completions.
    pub stop: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepSeekResponseFormat {
    JsonObject,
}

/// Request for DeepSeek beta FIM completion.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeepSeekFimRequest {
    pub model: Option<String>,
    pub prompt: String,
    pub suffix: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekFimResponse {
    pub id: Option<String>,
    pub object: Option<String>,
    pub created: Option<u64>,
    pub model: Option<String>,
    pub choices: Vec<DeepSeekFimChoice>,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekFimChoice {
    pub index: usize,
    pub text: String,
    pub finish_reason: Option<String>,
    pub logprobs: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekModelList {
    pub object: Option<String>,
    pub data: Vec<DeepSeekModel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekModel {
    pub id: String,
    pub object: Option<String>,
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekBalance {
    pub is_available: bool,
    pub balance_infos: Vec<DeepSeekBalanceInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekBalanceInfo {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}
