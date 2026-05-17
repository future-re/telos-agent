//! OpenAI Chat Completions API provider.
//!
//! Speaks `/v1/chat/completions` and its SSE streaming variant. Compatible
//! with any service that implements the same wire format (Azure OpenAI,
//! together.ai, Groq, etc.) — override `base_url` to point elsewhere.

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

/// Configuration for [`OpenAIProvider`].
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub model: String,
    /// Base URL — override to talk to an OpenAI-compatible service.
    pub base_url: String,
}

impl OpenAIConfig {
    /// Build a config from `OPENAI_API_KEY` and the given model.
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

/// [`ModelProvider`] implementation backed by OpenAI's Chat Completions API.
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

    /// Full URL for the `/v1/chat/completions` endpoint.
    fn endpoint(&self) -> String {
        format!(
            "{}/v1/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
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

    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        Box::pin(try_stream! {
            // Re-serialise the base request body, flip `stream: true`, and ask
            // for usage stats on the final chunk (otherwise we'd get none).
            let mut body = serde_json::to_value(openai_request_from_completion(&request, &self.config))
                .map_err(|err| AgentError::Provider(format!("invalid openai stream request: {err}")))?;
            body["stream"] = Value::Bool(true);
            body["stream_options"] = json!({ "include_usage": true });
            let response = self
                .client
                .post(self.endpoint())
                .bearer_auth(&self.config.api_key)
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
                Err(AgentError::Provider(format!("openai stream api error {}: {}", status, text)))?;
            } else {
                yield ProviderEvent::MessageStart;
                let mut sse = crate::provider::sse_data_stream(response);
                // OpenAI streams tool calls in indexed deltas — the same call's
                // fragments share an `index`, so we accumulate them positionally.
                let mut tool_calls: Vec<StreamingOpenAIToolCall> = Vec::new();
                let mut stop_reason = StopReason::EndTurn;
                let mut usage = None;

                while let Some(data) = sse.next().await {
                    let event: OpenAIStreamChunk = serde_json::from_str(&data?)
                        .map_err(|err| AgentError::Provider(format!("invalid openai stream event: {err}")))?;
                    if let Some(event_usage) = event.usage {
                        usage = Some(TokenUsage {
                            input_tokens: event_usage.prompt_tokens,
                            output_tokens: event_usage.completion_tokens,
                        });
                    }

                    for choice in event.choices {
                        if let Some(content) = choice.delta.content {
                            if !content.is_empty() {
                                yield ProviderEvent::TextDelta(content);
                            }
                        }
                        if let Some(delta_tool_calls) = choice.delta.tool_calls {
                            for delta_call in delta_tool_calls {
                                let index = delta_call.index;
                                // Grow the buffer so `index` is addressable.
                                while tool_calls.len() <= index {
                                    tool_calls.push(StreamingOpenAIToolCall::default());
                                }
                                let aggregate = &mut tool_calls[index];
                                if let Some(id) = delta_call.id {
                                    aggregate.id = id;
                                }
                                if let Some(function) = delta_call.function {
                                    if let Some(name) = function.name {
                                        aggregate.name = name;
                                    }
                                    if let Some(arguments) = function.arguments {
                                        // Arguments come in as partial JSON strings — concat them.
                                        aggregate.arguments.push_str(&arguments);
                                    }
                                }
                            }
                        }
                        if let Some(reason) = choice.finish_reason {
                            stop_reason = if reason == "tool_calls" {
                                StopReason::ToolUse
                            } else {
                                StopReason::EndTurn
                            };
                        }
                    }
                }

                // SSE stream finished — flush every fully-assembled tool call.
                for call in tool_calls {
                    if call.id.is_empty() && call.name.is_empty() {
                        continue;
                    }
                    let arguments = if call.arguments.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&call.arguments)
                            .map_err(|err| AgentError::Provider(format!("invalid openai streamed tool arguments: {err}")))?
                    };
                    yield ProviderEvent::ToolCall(ToolCall {
                        id: call.id,
                        name: call.name,
                        arguments,
                    });
                }

                yield ProviderEvent::MessageStop {
                    stop_reason,
                    usage,
                };
            }
        })
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        // OpenAI models (GPT-4, GPT-3.5) use cl100k_base; ~4 chars per token is a rough heuristic.
        (text.len() as f64 / 4.0).ceil() as usize
    }
}

/// Scratch state for an in-flight tool call assembled from streaming deltas.
#[derive(Debug, Default)]
struct StreamingOpenAIToolCall {
    id: String,
    name: String,
    /// Raw JSON-encoded arguments concatenated across `arguments` deltas.
    arguments: String,
}

// === OpenAI wire-format types =====================================================
// Private structs that mirror OpenAI's Chat Completions JSON shapes. Each
// field maps 1:1 to a wire field, so individual fields are left undocumented.

/// One SSE chunk delivered while streaming a completion.
#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    #[serde(default)]
    choices: Vec<OpenAIStreamChoice>,
    #[serde(default)]
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    delta: OpenAIStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAIStreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamToolCall {
    /// Position of this call within the assistant message — multiple streamed deltas share an index.
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAIStreamFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamFunctionCall {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// Top-level body of a `POST /v1/chat/completions` request.
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

/// Single message in the chat-completions transcript.
///
/// `tool_calls` is only populated on assistant messages; `tool_call_id` is
/// only populated on `tool`-role messages.
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
    /// JSON-encoded arguments — yes, a string, not a nested object (OpenAI quirk).
    arguments: String,
}

/// Decoded response body from `POST /v1/chat/completions` (non-streaming).
#[derive(Debug, Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: usize,
    completion_tokens: usize,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
    finish_reason: Option<String>,
}

/// Build the `/v1/chat/completions` request body from a generic [`CompletionRequest`].
///
/// OpenAI represents the system prompt as a regular message; prepend it here
/// rather than passing it separately like Anthropic.
fn openai_request_from_completion(
    request: &CompletionRequest,
    config: &OpenAIConfig,
) -> OpenAIRequestBody {
    let mut messages = request
        .messages
        .iter()
        .flat_map(message_to_openai)
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

/// Convert one of our [`Message`]s into OpenAI's role/content shape.
///
/// System messages are skipped (handled separately as a top-level prompt
/// prepended by [`openai_request_from_completion`]). Tool-role messages map
/// to OpenAI's `role: "tool"` with `tool_call_id`; only the first result in a
/// batch is emitted because OpenAI requires one tool message per call.
fn message_to_openai(message: &Message) -> Vec<OpenAIMessage> {
    match message.role {
        Role::System => vec![],
        Role::User => vec![OpenAIMessage {
            role: "user".into(),
            content: Some(message.text_content()),
            tool_calls: None,
            tool_call_id: None,
        }],
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
            vec![OpenAIMessage {
                role: "assistant".into(),
                content: (!message.text_content().is_empty()).then(|| message.text_content()),
                tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
                tool_call_id: None,
            }]
        }
        Role::Tool => message
            .tool_results_iter()
            .map(|result| OpenAIMessage {
                role: "tool".into(),
                content: Some(result.content.to_string()),
                tool_calls: None,
                tool_call_id: Some(result.tool_call_id.clone()),
            })
            .collect(),
    }
}

/// Translate OpenAI's response into the crate-level [`CompletionResponse`].
///
/// Only the first choice is consumed — we don't support `n > 1` in
/// [`CompletionRequest`].
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
            let arguments: Value =
                serde_json::from_str(&call.function.arguments).map_err(|err| {
                    AgentError::Provider(format!("invalid openai tool arguments: {err}"))
                })?;
            blocks.push(ContentBlock::ToolCall(ToolCall {
                id: call.id,
                name: call.function.name,
                arguments,
            }));
        }
    }

    let usage = response.usage.map(|u| TokenUsage {
        input_tokens: u.prompt_tokens,
        output_tokens: u.completion_tokens,
    });

    Ok(CompletionResponse {
        message: Message {
            role: Role::Assistant,
            blocks,
        },
        stop_reason: match choice.finish_reason.as_deref() {
            Some("tool_calls") => StopReason::ToolUse,
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
            usage: None,
        };

        let completion = openai_response_to_completion(response).unwrap();
        assert_eq!(completion.stop_reason, StopReason::ToolUse);
        assert_eq!(completion.message.tool_calls().count(), 1);
    }

    #[test]
    fn builds_one_openai_tool_message_per_tool_result() {
        let request = CompletionRequest {
            system_prompt: None,
            messages: vec![
                Message {
                    role: Role::Assistant,
                    blocks: vec![
                        ContentBlock::ToolCall(ToolCall {
                            id: "call-1".into(),
                            name: "Read".into(),
                            arguments: json!({ "file_path": "a.txt" }),
                        }),
                        ContentBlock::ToolCall(ToolCall {
                            id: "call-2".into(),
                            name: "Read".into(),
                            arguments: json!({ "file_path": "b.txt" }),
                        }),
                    ],
                },
                Message::tool_results(vec![
                    ToolResult {
                        tool_call_id: "call-1".into(),
                        name: "Read".into(),
                        content: json!({ "content": "a" }),
                        is_error: false,
                    },
                    ToolResult {
                        tool_call_id: "call-2".into(),
                        name: "Read".into(),
                        content: json!({ "content": "b" }),
                        is_error: false,
                    },
                ]),
            ],
            tools: vec![ToolDefinition {
                name: "Read".into(),
                description: "Read file".into(),
                input_schema: json!({ "type": "object" }),
            }],
        };
        let config = OpenAIConfig {
            api_key: "key".into(),
            model: "gpt-4".into(),
            base_url: "https://api.openai.com".into(),
        };

        let body = openai_request_from_completion(&request, &config);
        let tool_messages = body
            .messages
            .iter()
            .filter(|message| message.role == "tool")
            .collect::<Vec<_>>();
        assert_eq!(tool_messages.len(), 2);
        assert_eq!(tool_messages[0].tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(tool_messages[1].tool_call_id.as_deref(), Some("call-2"));
    }
}
