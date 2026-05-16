use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AgentError;
use crate::message::{ContentBlock, Message, Role, TextBlock, ToolCall};
use crate::tool::ToolDefinition;

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

#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub base_url: String,
    pub anthropic_version: String,
}

impl AnthropicConfig {
    pub fn from_env(model: impl Into<String>, max_tokens: u32) -> Result<Self, AgentError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| AgentError::Config("missing ANTHROPIC_API_KEY".into()))?;

        Ok(Self {
            api_key,
            model: model.into(),
            max_tokens,
            base_url: "https://api.anthropic.com".into(),
            anthropic_version: "2023-06-01".into(),
        })
    }
}

pub struct AnthropicProvider {
    client: Client,
    config: AnthropicConfig,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.config.base_url.trim_end_matches('/'))
    }
}

#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

impl OpenAIConfig {
    pub fn from_env(model: impl Into<String>) -> Result<Self, AgentError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| AgentError::Config("missing OPENAI_API_KEY".into()))?;

        Ok(Self {
            api_key,
            model: model.into(),
            base_url: "https://api.openai.com".into(),
        })
    }
}

pub struct OpenAIProvider {
    client: Client,
    config: OpenAIConfig,
}

impl OpenAIProvider {
    pub fn new(config: OpenAIConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/v1/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError> {
        let body = anthropic_request_from_completion(&request, &self.config);
        let response = self
            .client
            .post(self.endpoint())
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", &self.config.anthropic_version)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|err| AgentError::Provider(err.to_string()))?;

        let status = response.status();
        let payload = response
            .bytes()
            .await
            .map_err(|err| AgentError::Provider(err.to_string()))?;

        if !status.is_success() {
            let text = String::from_utf8_lossy(&payload);
            return Err(AgentError::Provider(format!(
                "anthropic api error {}: {}",
                status, text
            )));
        }

        let decoded: AnthropicMessageResponse = serde_json::from_slice(&payload)
            .map_err(|err| AgentError::Provider(format!("invalid anthropic response: {err}")))?;

        anthropic_response_to_completion(decoded)
    }
}

#[async_trait]
impl ModelProvider for OpenAIProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError> {
        let body = openai_request_from_completion(&request, &self.config);
        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|err| AgentError::Provider(err.to_string()))?;

        let status = response.status();
        let payload = response
            .bytes()
            .await
            .map_err(|err| AgentError::Provider(err.to_string()))?;

        if !status.is_success() {
            let text = String::from_utf8_lossy(&payload);
            return Err(AgentError::Provider(format!(
                "openai api error {}: {}",
                status, text
            )));
        }

        let decoded: OpenAIChatResponse = serde_json::from_slice(&payload)
            .map_err(|err| AgentError::Provider(format!("invalid openai response: {err}")))?;

        openai_response_to_completion(decoded)
    }
}

#[derive(Debug, Serialize)]
struct AnthropicRequestBody {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<AnthropicTool>,
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: Value },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageResponse {
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAIRequestBody {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIFunctionDefinition,
}

#[derive(Debug, Serialize)]
struct OpenAIFunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIFunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
    finish_reason: Option<String>,
}

fn anthropic_request_from_completion(
    request: &CompletionRequest,
    config: &AnthropicConfig,
) -> AnthropicRequestBody {
    let messages = request
        .messages
        .iter()
        .filter_map(message_to_anthropic)
        .collect::<Vec<_>>();

    let tools = request
        .tools
        .iter()
        .map(|tool| AnthropicTool {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
        })
        .collect::<Vec<_>>();

    AnthropicRequestBody {
        model: config.model.clone(),
        max_tokens: config.max_tokens,
        messages,
        system: request.system_prompt.clone(),
        tools,
    }
}

fn openai_request_from_completion(
    request: &CompletionRequest,
    config: &OpenAIConfig,
) -> OpenAIRequestBody {
    let mut messages = request
        .messages
        .iter()
        .filter_map(message_to_openai)
        .collect::<Vec<_>>();
    if let Some(system_prompt) = &request.system_prompt {
        messages.insert(
            0,
            OpenAIMessage {
                role: "system".into(),
                content: Some(system_prompt.clone()),
                tool_calls: None,
                tool_call_id: None,
            },
        );
    }

    let tools = (!request.tools.is_empty()).then(|| {
        request
            .tools
            .iter()
            .map(|tool| OpenAITool {
                tool_type: "function".into(),
                function: OpenAIFunctionDefinition {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.input_schema.clone(),
                },
            })
            .collect()
    });

    OpenAIRequestBody {
        model: config.model.clone(),
        messages,
        tools,
    }
}

fn message_to_anthropic(message: &Message) -> Option<AnthropicMessage> {
    match message.role {
        Role::System => None,
        Role::User => Some(AnthropicMessage {
            role: "user".into(),
            content: text_blocks_for_anthropic(message),
        }),
        Role::Assistant => Some(AnthropicMessage {
            role: "assistant".into(),
            content: assistant_blocks_for_anthropic(message),
        }),
        Role::Tool => Some(AnthropicMessage {
            role: "user".into(),
            content: tool_result_blocks_for_anthropic(message),
        }),
    }
}

fn message_to_openai(message: &Message) -> Option<OpenAIMessage> {
    match message.role {
        Role::System => None,
        Role::User => Some(OpenAIMessage {
            role: "user".into(),
            content: Some(message.text_content()),
            tool_calls: None,
            tool_call_id: None,
        }),
        Role::Assistant => {
            let tool_calls = message
                .tool_calls()
                .map(|call| OpenAIToolCall {
                    id: call.id.clone(),
                    tool_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: call.name.clone(),
                        arguments: call.arguments.to_string(),
                    },
                })
                .collect::<Vec<_>>();
            Some(OpenAIMessage {
                role: "assistant".into(),
                content: (!message.text_content().is_empty()).then(|| message.text_content()),
                tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
                tool_call_id: None,
            })
        }
        Role::Tool => {
            let first = message.tool_results_iter().next()?;
            Some(OpenAIMessage {
                role: "tool".into(),
                content: Some(first.content.to_string()),
                tool_calls: None,
                tool_call_id: Some(first.tool_call_id.clone()),
            })
        }
    }
}

fn text_blocks_for_anthropic(message: &Message) -> Vec<AnthropicContentBlock> {
    message
        .blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(TextBlock { text }) => Some(AnthropicContentBlock::Text {
                text: text.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn assistant_blocks_for_anthropic(message: &Message) -> Vec<AnthropicContentBlock> {
    message
        .blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(TextBlock { text }) => Some(AnthropicContentBlock::Text {
                text: text.clone(),
            }),
            ContentBlock::ToolCall(ToolCall {
                id,
                name,
                arguments,
            }) => Some(AnthropicContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: arguments.clone(),
            }),
            ContentBlock::ToolResult(_) => None,
        })
        .collect()
}

fn tool_result_blocks_for_anthropic(message: &Message) -> Vec<AnthropicContentBlock> {
    message
        .tool_results_iter()
        .map(|result| AnthropicContentBlock::ToolResult {
            tool_use_id: result.tool_call_id.clone(),
            content: result.content.clone(),
            is_error: result.is_error.then_some(true),
        })
        .collect()
}

fn anthropic_response_to_completion(
    response: AnthropicMessageResponse,
) -> Result<CompletionResponse, AgentError> {
    let mut blocks = Vec::new();

    for block in response.content {
        match block {
            AnthropicContentBlock::Text { text } => {
                blocks.push(ContentBlock::Text(TextBlock { text }))
            }
            AnthropicContentBlock::ToolUse { id, name, input } => {
                blocks.push(ContentBlock::ToolCall(ToolCall {
                    id,
                    name,
                    arguments: input,
                }))
            }
            AnthropicContentBlock::ToolResult { .. } => {
                return Err(AgentError::Provider(
                    "anthropic assistant response unexpectedly contained tool_result".into(),
                ))
            }
        }
    }

    Ok(CompletionResponse {
        message: Message {
            role: Role::Assistant,
            blocks,
        },
        stop_reason: match response.stop_reason.as_deref() {
            Some("tool_use") => StopReason::ToolUse,
            _ => StopReason::EndTurn,
        },
    })
}

fn openai_response_to_completion(
    response: OpenAIChatResponse,
) -> Result<CompletionResponse, AgentError> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| AgentError::Provider("openai response missing choice".into()))?;
    let mut blocks = Vec::new();
    if let Some(content) = choice.message.content {
        if !content.is_empty() {
            blocks.push(ContentBlock::Text(TextBlock { text: content }));
        }
    }
    if let Some(tool_calls) = choice.message.tool_calls {
        for call in tool_calls {
            let arguments: Value = serde_json::from_str(&call.function.arguments)
                .map_err(|err| AgentError::Provider(format!("invalid openai tool arguments: {err}")))?;
            blocks.push(ContentBlock::ToolCall(ToolCall {
                id: call.id,
                name: call.function.name,
                arguments,
            }));
        }
    }

    Ok(CompletionResponse {
        message: Message {
            role: Role::Assistant,
            blocks,
        },
        stop_reason: match choice.finish_reason.as_deref() {
            Some("tool_calls") => StopReason::ToolUse,
            _ => StopReason::EndTurn,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ToolResult;
    use crate::tool::ToolDefinition;
    use serde_json::json;

    #[test]
    fn builds_anthropic_request_with_tool_results_as_user_message() {
        let request = CompletionRequest {
            system_prompt: Some("You are helpful.".into()),
            messages: vec![
                Message::system("You are helpful."),
                Message::user("What is 2 + 3?"),
                Message {
                    role: Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 2, "b": 3 }),
                    })],
                },
                Message::tool_results(vec![ToolResult {
                    tool_call_id: "call-1".into(),
                    name: "add".into(),
                    content: json!({ "sum": 5 }),
                    is_error: false,
                }]),
            ],
            tools: vec![ToolDefinition {
                name: "add".into(),
                description: "Add two integers".into(),
                input_schema: json!({ "type": "object" }),
            }],
        };
        let config = AnthropicConfig {
            api_key: "key".into(),
            model: "claude-sonnet-4-5".into(),
            max_tokens: 1024,
            base_url: "https://api.anthropic.com".into(),
            anthropic_version: "2023-06-01".into(),
        };

        let body = anthropic_request_from_completion(&request, &config);

        assert_eq!(body.messages.len(), 3);
        assert_eq!(body.messages[0].role, "user");
        assert_eq!(body.messages[1].role, "assistant");
        assert_eq!(body.messages[2].role, "user");
    }

    #[test]
    fn parses_anthropic_tool_use_response() {
        let response = AnthropicMessageResponse {
            content: vec![
                AnthropicContentBlock::Text {
                    text: "Let me calculate that.".into(),
                },
                AnthropicContentBlock::ToolUse {
                    id: "call-1".into(),
                    name: "add".into(),
                    input: json!({ "a": 2, "b": 3 }),
                },
            ],
            stop_reason: Some("tool_use".into()),
        };

        let completion = anthropic_response_to_completion(response).unwrap();

        assert_eq!(completion.stop_reason, StopReason::ToolUse);
        assert_eq!(completion.message.tool_calls().count(), 1);
    }

    #[test]
    fn parses_openai_tool_call_response() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChoice {
                message: OpenAIMessage {
                    role: "assistant".into(),
                    content: Some("Let me calculate that.".into()),
                    tool_calls: Some(vec![OpenAIToolCall {
                        id: "call-1".into(),
                        tool_type: "function".into(),
                        function: OpenAIFunctionCall {
                            name: "add".into(),
                            arguments: "{\"a\":2,\"b\":3}".into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".into()),
            }],
        };

        let completion = openai_response_to_completion(response).unwrap();
        assert_eq!(completion.stop_reason, StopReason::ToolUse);
        assert_eq!(completion.message.tool_calls().count(), 1);
    }
}
