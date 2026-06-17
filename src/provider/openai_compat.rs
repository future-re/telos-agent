//! Shared helpers for OpenAI-compatible chat-completions providers.
//!
//! Both Kimi and DeepSeek expose the same `/v1/chat/completions` shape, so the
//! request building and response parsing logic lives here to avoid duplication.

use async_openai::config::OpenAIConfig as AsyncOpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageContent,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestSystemMessageContent, ChatCompletionRequestToolMessage,
    ChatCompletionRequestToolMessageContent, ChatCompletionRequestUserMessage,
    ChatCompletionRequestUserMessageContent, ChatCompletionResponseMessage,
    ChatCompletionStreamOptions, ChatCompletionTool, ChatCompletionTools,
    CreateChatCompletionRequest, CreateChatCompletionResponse,
    CreateChatCompletionStreamResponse, FinishReason, FunctionCall, FunctionObject,
};
use async_openai::Client;
use async_stream::try_stream;
use futures_core::stream::Stream;
use futures_util::StreamExt;
use serde_json::Value;

use crate::error::AgentError;
use crate::message::{ContentBlock, Message, Role, TextBlock, ToolCall, ToolResult};
use crate::provider::{CompletionRequest, CompletionResponse, ProviderEvent, StopReason, TokenUsage};
use crate::tool::ToolDefinition;

/// Build an [`async_openai`] client configured for a custom base URL.
pub(crate) fn build_client(api_key: &str, base_url: &str) -> Client<AsyncOpenAIConfig> {
    // async-openai expects `api_base` to end with `/v1`; the chat path is `/chat/completions`.
    let api_base = normalize_api_base(base_url);
    let config = AsyncOpenAIConfig::new()
        .with_api_key(api_key)
        .with_api_base(api_base);
    Client::with_config(config)
}

fn normalize_api_base(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{}/v1", trimmed)
    }
}

/// Build a [`CreateChatCompletionRequest`] from the provider-agnostic request.
pub(crate) fn build_request(
    model: &str,
    request: CompletionRequest,
) -> CreateChatCompletionRequest {
    let mut messages: Vec<ChatCompletionRequestMessage> = request
        .messages
        .iter()
        .flat_map(message_to_openai)
        .collect();

    if let Some(system_prompt) = &request.system_prompt {
        messages.insert(
            0,
            ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                content: ChatCompletionRequestSystemMessageContent::Text(system_prompt.clone()),
                name: None,
            }),
        );
    }

    let tools = (!request.tools.is_empty()).then(|| {
        request
            .tools
            .iter()
            .map(tool_to_openai)
            .map(ChatCompletionTools::Function)
            .collect()
    });

    CreateChatCompletionRequest {
        model: model.to_string(),
        messages,
        tools,
        ..Default::default()
    }
}

fn tool_to_openai(tool: &ToolDefinition) -> ChatCompletionTool {
    ChatCompletionTool {
        function: FunctionObject {
            name: tool.name.clone(),
            description: Some(tool.description.clone()),
            parameters: Some(tool.input_schema.clone()),
            strict: None,
        },
    }
}

fn message_to_openai(message: &Message) -> Vec<ChatCompletionRequestMessage> {
    match message.role {
        Role::System => vec![],
        Role::User => vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: ChatCompletionRequestUserMessageContent::Text(message.text_content()),
                name: None,
            },
        )],
        Role::Assistant => {
            let text = message.text_content();
            let content = if text.is_empty() {
                None
            } else {
                Some(ChatCompletionRequestAssistantMessageContent::Text(text))
            };

            let tool_calls: Vec<ChatCompletionMessageToolCalls> = message
                .tool_calls()
                .map(tool_call_to_openai)
                .collect();

            vec![ChatCompletionRequestMessage::Assistant(
                ChatCompletionRequestAssistantMessage {
                    content,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    ..Default::default()
                },
            )]
        }
        Role::Tool => message
            .tool_results_iter()
            .map(tool_result_to_openai)
            .collect(),
    }
}

fn tool_call_to_openai(call: &ToolCall) -> ChatCompletionMessageToolCalls {
    ChatCompletionMessageToolCalls::Function(ChatCompletionMessageToolCall {
        id: call.id.clone(),
        function: FunctionCall {
            name: call.name.clone(),
            arguments: serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".into()),
        },
    })
}

fn tool_result_to_openai(result: &ToolResult) -> ChatCompletionRequestMessage {
    ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
        content: ChatCompletionRequestToolMessageContent::Text(result.content.to_string()),
        tool_call_id: result.tool_call_id.clone(),
    })
}

/// Parse an [`async_openai`] response into the crate-level completion result.
pub(crate) fn parse_response(
    response: CreateChatCompletionResponse,
) -> Result<CompletionResponse, AgentError> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| AgentError::Provider("provider response missing choice".into()))?;

    let message = parse_response_message(choice.message)?;
    let stop_reason = match choice.finish_reason {
        Some(FinishReason::ToolCalls) => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    };
    let usage = response.usage.map(|u| TokenUsage {
        input_tokens: u.prompt_tokens as usize,
        output_tokens: u.completion_tokens as usize,
    });

    Ok(CompletionResponse {
        message,
        stop_reason,
        usage,
    })
}

fn parse_response_message(
    message: ChatCompletionResponseMessage,
) -> Result<Message, AgentError> {
    let mut blocks = Vec::new();

    if let Some(content) = message.content {
        if !content.is_empty() {
            blocks.push(ContentBlock::Text(TextBlock { text: content }));
        }
    }

    if let Some(tool_calls) = message.tool_calls {
        for call in tool_calls {
            match call {
                ChatCompletionMessageToolCalls::Function(func) => {
                    let arguments: Value = serde_json::from_str(&func.function.arguments)
                        .map_err(|err| {
                            AgentError::Provider(format!("invalid tool arguments: {err}"))
                        })?;
                    blocks.push(ContentBlock::ToolCall(ToolCall {
                        id: func.id,
                        name: func.function.name,
                        arguments,
                    }));
                }
                ChatCompletionMessageToolCalls::Custom(_) => {
                    // Custom tools are not supported by this crate.
                }
            }
        }
    }

    Ok(Message {
        role: Role::Assistant,
        blocks,
    })
}

/// Stream a completion using [`async_openai`]'s SSE endpoint.
pub(crate) fn stream_complete(
    client: Client<AsyncOpenAIConfig>,
    model: String,
    request: CompletionRequest,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send>> {
    Box::pin(try_stream! {
        let mut openai_request = build_request(&model, request);
        openai_request.stream = Some(true);
        openai_request.stream_options = Some(ChatCompletionStreamOptions {
            include_usage: Some(true),
            include_obfuscation: None,
        });

        let mut stream = client
            .chat()
            .create_stream(openai_request)
            .await
            .map_err(|err| AgentError::Provider(err.to_string()))?;

        yield ProviderEvent::MessageStart;

        let mut tool_calls: Vec<StreamingToolCall> = Vec::new();
        let mut stop_reason = StopReason::EndTurn;
        let mut usage = None;

        while let Some(chunk) = stream.next().await {
            let chunk: CreateChatCompletionStreamResponse =
                chunk.map_err(|err| AgentError::Provider(err.to_string()))?;

            if let Some(chunk_usage) = chunk.usage {
                usage = Some(TokenUsage {
                    input_tokens: chunk_usage.prompt_tokens as usize,
                    output_tokens: chunk_usage.completion_tokens as usize,
                });
            }

            for choice in chunk.choices {
                if let Some(content) = choice.delta.content {
                    if !content.is_empty() {
                        yield ProviderEvent::TextDelta(content);
                    }
                }

                if let Some(delta_tool_calls) = choice.delta.tool_calls {
                    for delta in delta_tool_calls {
                        let index = delta.index as usize;
                        while tool_calls.len() <= index {
                            tool_calls.push(StreamingToolCall::default());
                        }
                        let aggregate = &mut tool_calls[index];
                        if let Some(id) = delta.id {
                            aggregate.id = id;
                        }
                        if let Some(function) = delta.function {
                            if let Some(name) = function.name {
                                aggregate.name = name;
                            }
                            if let Some(args) = function.arguments {
                                aggregate.arguments.push_str(&args);
                            }
                        }
                    }
                }

                if let Some(reason) = choice.finish_reason {
                    stop_reason = match reason {
                        FinishReason::ToolCalls => StopReason::ToolUse,
                        _ => StopReason::EndTurn,
                    };
                }
            }
        }

        for call in tool_calls {
            if call.id.is_empty() && call.name.is_empty() {
                continue;
            }
            let arguments = if call.arguments.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&call.arguments)
                    .map_err(|err| AgentError::Provider(format!("invalid streamed tool arguments: {err}")))?
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
    })
}

#[derive(Debug, Default)]
struct StreamingToolCall {
    id: String,
    name: String,
    arguments: String,
}
