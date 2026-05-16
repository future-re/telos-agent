use serde_json::json;

use crate::error::AgentError;
use crate::message::{ContentBlock, Message, ToolResult};
use crate::provider::{CompletionRequest, ModelProvider, StopReason};
use crate::tool::ToolRegistry;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub system_prompt: Option<String>,
    pub max_iterations: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: None,
            max_iterations: 8,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TurnEvent {
    User(Message),
    Assistant(Message),
    ToolResult(Message),
}

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub events: Vec<TurnEvent>,
    pub final_message: Message,
}

pub struct AgentSession {
    config: AgentConfig,
    messages: Vec<Message>,
}

impl AgentSession {
    pub fn new(config: AgentConfig) -> Self {
        let mut messages = Vec::new();
        if let Some(system_prompt) = config.system_prompt.as_ref() {
            messages.push(Message::system(system_prompt.clone()));
        }

        Self { config, messages }
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub async fn run_turn<P: ModelProvider>(
        &mut self,
        provider: &P,
        tools: &ToolRegistry,
        user_input: impl Into<String>,
    ) -> Result<TurnResult, AgentError> {
        let user_message = Message::user(user_input);
        self.messages.push(user_message.clone());

        let mut events = vec![TurnEvent::User(user_message)];
        let mut iterations = 0;

        loop {
            if iterations >= self.config.max_iterations {
                return Err(AgentError::MaxIterations(self.config.max_iterations));
            }
            iterations += 1;

            let response = provider
                .complete(CompletionRequest {
                    system_prompt: self.config.system_prompt.clone(),
                    messages: self.messages.clone(),
                    tools: tools.definitions(),
                })
                .await?;

            let assistant_message = response.message;
            self.messages.push(assistant_message.clone());
            events.push(TurnEvent::Assistant(assistant_message.clone()));

            if response.stop_reason != StopReason::ToolUse {
                return Ok(TurnResult {
                    events,
                    final_message: assistant_message,
                });
            }

            let tool_calls = assistant_message.tool_calls().cloned().collect::<Vec<_>>();
            if tool_calls.is_empty() {
                return Ok(TurnResult {
                    events,
                    final_message: assistant_message,
                });
            }

            let mut tool_results = Vec::with_capacity(tool_calls.len());
            for call in tool_calls {
                let result = match tools.invoke(&call.name, call.arguments.clone()).await {
                    Ok(output) => ToolResult {
                        tool_call_id: call.id,
                        name: call.name,
                        content: output.content,
                        is_error: false,
                    },
                    Err(err) => ToolResult {
                        tool_call_id: call.id,
                        name: call.name.clone(),
                        content: json!({ "error": err.to_string() }),
                        is_error: true,
                    },
                };
                tool_results.push(result);
            }

            let tool_message = Message::tool_results(tool_results);
            self.messages.push(tool_message.clone());
            events.push(TurnEvent::ToolResult(tool_message));
        }
    }
}

impl TurnEvent {
    pub fn message(&self) -> &Message {
        match self {
            TurnEvent::User(message)
            | TurnEvent::Assistant(message)
            | TurnEvent::ToolResult(message) => message,
        }
    }

    pub fn text(&self) -> String {
        self.message()
            .blocks
            .iter()
            .map(|block| match block {
                ContentBlock::Text(text) => text.text.clone(),
                ContentBlock::ToolCall(call) => {
                    format!("tool_call:{}({})", call.name, call.arguments)
                }
                ContentBlock::ToolResult(result) => {
                    format!("tool_result:{}={}", result.name, result.content)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
