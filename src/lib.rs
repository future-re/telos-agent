pub mod error;
pub mod message;
pub mod mock;
pub mod provider;
pub mod runtime;
pub mod tool;

pub use error::AgentError;
pub use message::{ContentBlock, Message, Role, TextBlock, ToolCall, ToolResult};
pub use mock::MockProvider;
pub use provider::{
    AnthropicConfig, AnthropicProvider, CompletionRequest, CompletionResponse, ModelProvider,
    StopReason,
};
pub use runtime::{AgentConfig, AgentSession, TurnEvent, TurnResult};
pub use tool::{Tool, ToolDefinition, ToolOutput, ToolRegistry};

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use serde_json::{Value, json};

    use crate::MockProvider;
    use crate::error::AgentError;
    use crate::message::{ContentBlock, Message, ToolCall};
    use crate::provider::{CompletionResponse, StopReason};
    use crate::runtime::{AgentConfig, AgentSession};
    use crate::tool::{Tool, ToolDefinition, ToolOutput, ToolRegistry};

    struct AddTool;

    #[async_trait]
    impl Tool for AddTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "add".into(),
                description: "Add two integers".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "a": { "type": "integer" },
                        "b": { "type": "integer" }
                    },
                    "required": ["a", "b"]
                }),
            }
        }

        async fn invoke(&self, arguments: Value) -> Result<ToolOutput, AgentError> {
            let a = arguments["a"]
                .as_i64()
                .ok_or_else(|| AgentError::ToolExecution {
                    tool: "add".into(),
                    message: "missing integer `a`".into(),
                })?;
            let b = arguments["b"]
                .as_i64()
                .ok_or_else(|| AgentError::ToolExecution {
                    tool: "add".into(),
                    message: "missing integer `b`".into(),
                })?;

            Ok(ToolOutput {
                content: json!({ "sum": a + b }),
            })
        }
    }

    #[test]
    fn multi_step_tool_loop_completes() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let provider = MockProvider::new(vec![
                CompletionResponse {
                    message: Message {
                        role: crate::Role::Assistant,
                        blocks: vec![
                            ContentBlock::Text(crate::TextBlock {
                                text: "Let me calculate that.".into(),
                            }),
                            ContentBlock::ToolCall(ToolCall {
                                id: "call-1".into(),
                                name: "add".into(),
                                arguments: json!({ "a": 2, "b": 3 }),
                            }),
                        ],
                    },
                    stop_reason: StopReason::ToolUse,
                },
                CompletionResponse {
                    message: Message::assistant("The answer is 5."),
                    stop_reason: StopReason::EndTurn,
                },
            ]);

            let mut tools = ToolRegistry::new();
            tools.register(AddTool);

            let mut session = AgentSession::new(AgentConfig {
                system_prompt: Some("You are a coding agent.".into()),
                max_iterations: 4,
            });

            let result = session
                .run_turn(&provider, &tools, "what is 2 + 3?")
                .await
                .unwrap();
            assert_eq!(result.final_message.text_content(), "The answer is 5.");
            assert_eq!(result.events.len(), 4);
            assert_eq!(session.messages().len(), 5);
        });
    }

    #[test]
    fn missing_tool_returns_error_result_message() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let provider = MockProvider::new(vec![
                CompletionResponse {
                    message: Message {
                        role: crate::Role::Assistant,
                        blocks: vec![ContentBlock::ToolCall(ToolCall {
                            id: "call-1".into(),
                            name: "missing".into(),
                            arguments: json!({}),
                        })],
                    },
                    stop_reason: StopReason::ToolUse,
                },
                CompletionResponse {
                    message: Message::assistant("I could not run that tool."),
                    stop_reason: StopReason::EndTurn,
                },
            ]);

            let tools = ToolRegistry::new();
            let mut session = AgentSession::new(AgentConfig::default());

            let result = session
                .run_turn(&provider, &tools, "try a tool")
                .await
                .unwrap();
            let tool_result_event = result
                .events
                .iter()
                .find(|event| matches!(event, crate::TurnEvent::ToolResult(_)));

            assert!(tool_result_event.is_some());
            assert!(
                tool_result_event
                    .unwrap()
                    .text()
                    .contains("tool not found: missing")
            );
        });
    }
}
