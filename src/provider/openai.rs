use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AgentError;
use crate::message::{ContentBlock, Message, Role, TextBlock, ToolCall};
use crate::provider::{CompletionRequest, CompletionResponse, ModelProvider, StopReason};

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
impl ModelProvider for OpenAIProvider {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, AgentError> {
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
