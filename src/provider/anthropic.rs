use async_stream::try_stream;
use async_trait::async_trait;
use futures_core::stream::Stream;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::message::{ContentBlock, Message, Role, TextBlock, ToolCall};
use crate::provider::{
    CompletionRequest, CompletionResponse, ModelProvider, ProviderEvent, StopReason, TokenUsage,
};

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

    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        Box::pin(try_stream! {
            let mut body = serde_json::to_value(anthropic_request_from_completion(&request, &self.config))
                .map_err(|err| AgentError::Provider(format!("invalid anthropic stream request: {err}")))?;
            body["stream"] = Value::Bool(true);
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

            if !response.status().is_success() {
                let status = response.status();
                let text = response
                    .text()
                    .await
                    .map_err(|err| AgentError::Provider(err.to_string()))?;
                Err(AgentError::Provider(format!("anthropic stream api error {}: {}", status, text)))?;
            } else {
                yield ProviderEvent::MessageStart;
                let mut sse = crate::provider::sse_data_stream(response);
                let mut current_tool: Option<StreamingAnthropicToolCall> = None;
                let mut usage = TokenUsage::default();
                let mut stop_reason = StopReason::EndTurn;

                while let Some(data) = sse.next().await {
                    let event: AnthropicStreamEvent = serde_json::from_str(&data?)
                        .map_err(|err| AgentError::Provider(format!("invalid anthropic stream event: {err}")))?;
                    match event.event_type.as_str() {
                        "message_start" => {
                            if let Some(message) = event.message {
                                if let Some(event_usage) = message.usage {
                                    usage.input_tokens = event_usage.input_tokens.unwrap_or(0);
                                    usage.output_tokens = event_usage.output_tokens.unwrap_or(0);
                                }
                            }
                        }
                        "content_block_start" => {
                            if let Some(block) = event.content_block {
                                match block.block_type.as_str() {
                                    "text" => {
                                        if let Some(text) = block.text {
                                            if !text.is_empty() {
                                                yield ProviderEvent::TextDelta(text);
                                            }
                                        }
                                    }
                                    "tool_use" => {
                                        current_tool = Some(StreamingAnthropicToolCall {
                                            id: block.id.unwrap_or_default(),
                                            name: block.name.unwrap_or_default(),
                                            json_input: String::new(),
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                        "content_block_delta" => {
                            if let Some(delta) = event.delta {
                                match delta.delta_type.as_deref() {
                                    Some("text_delta") => {
                                        if let Some(text) = delta.text {
                                            yield ProviderEvent::TextDelta(text);
                                        }
                                    }
                                    Some("input_json_delta") => {
                                        if let (Some(tool), Some(partial)) = (&mut current_tool, delta.partial_json) {
                                            tool.json_input.push_str(&partial);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        "content_block_stop" => {
                            if let Some(tool) = current_tool.take() {
                                let arguments = if tool.json_input.trim().is_empty() {
                                    json!({})
                                } else {
                                    serde_json::from_str(&tool.json_input)
                                        .map_err(|err| AgentError::Provider(format!("invalid anthropic streamed tool input: {err}")))?
                                };
                                yield ProviderEvent::ToolCall(ToolCall {
                                    id: tool.id,
                                    name: tool.name,
                                    arguments,
                                });
                            }
                        }
                        "message_delta" => {
                            if let Some(delta) = event.delta {
                                if let Some(reason) = delta.stop_reason {
                                    stop_reason = if reason == "tool_use" {
                                        StopReason::ToolUse
                                    } else {
                                        StopReason::EndTurn
                                    };
                                }
                            }
                            if let Some(event_usage) = event.usage {
                                if let Some(output_tokens) = event_usage.output_tokens {
                                    usage.output_tokens = output_tokens;
                                }
                            }
                        }
                        "message_stop" => break,
                        _ => {}
                    }
                }

                yield ProviderEvent::MessageStop {
                    stop_reason,
                    usage: Some(usage),
                };
            }
        })
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        // Claude uses a SentencePiece-derived tokenizer; ~3.5 chars per token is a rough heuristic.
        (text.len() as f64 / 3.5).ceil() as usize
    }
}

#[derive(Debug)]
struct StreamingAnthropicToolCall {
    id: String,
    name: String,
    json_input: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    message: Option<AnthropicStreamMessage>,
    #[serde(default)]
    content_block: Option<AnthropicStreamContentBlock>,
    #[serde(default)]
    delta: Option<AnthropicStreamDelta>,
    #[serde(default)]
    usage: Option<AnthropicStreamUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamMessage {
    #[serde(default)]
    usage: Option<AnthropicStreamUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamDelta {
    #[serde(rename = "type", default)]
    delta_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamUsage {
    #[serde(default)]
    input_tokens: Option<usize>,
    #[serde(default)]
    output_tokens: Option<usize>,
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
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
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
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: usize,
    output_tokens: usize,
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

fn text_blocks_for_anthropic(message: &Message) -> Vec<AnthropicContentBlock> {
    message
        .blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(TextBlock { text }) => {
                Some(AnthropicContentBlock::Text { text: text.clone() })
            }
            _ => None,
        })
        .collect()
}

fn assistant_blocks_for_anthropic(message: &Message) -> Vec<AnthropicContentBlock> {
    message
        .blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(TextBlock { text }) => {
                Some(AnthropicContentBlock::Text { text: text.clone() })
            }
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
                ));
            }
        }
    }

    let usage = response.usage.map(|u| TokenUsage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
    });

    Ok(CompletionResponse {
        message: Message {
            role: Role::Assistant,
            blocks,
        },
        stop_reason: match response.stop_reason.as_deref() {
            Some("tool_use") => StopReason::ToolUse,
            _ => StopReason::EndTurn,
        },
        usage,
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
            usage: None,
        };

        let completion = anthropic_response_to_completion(response).unwrap();

        assert_eq!(completion.stop_reason, StopReason::ToolUse);
        assert_eq!(completion.message.tool_calls().count(), 1);
    }
}
