pub mod compaction;
pub mod error;
pub mod hooks;
pub mod message;
pub mod mock;
pub mod provider;
pub mod runtime;
pub mod tool;

pub use error::AgentError;
pub use hooks::{Hook, HookContext, HookPhase, HookRegistry};
pub use message::{ContentBlock, Message, Role, TextBlock, ToolCall, ToolResult};
pub use mock::MockProvider;
pub use provider::{
    AnthropicConfig, AnthropicProvider, CompletionRequest, CompletionResponse, ModelProvider,
    OpenAIConfig, OpenAIProvider, StopReason,
};
pub use runtime::{AgentConfig, AgentSession, TurnEvent, TurnResult};
pub use tool::{
    InterruptBehavior, PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput,
    ToolRegistry,
};

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use futures_util::StreamExt;
    use serde_json::{Value, json};
    use std::sync::Arc;

    use crate::error::AgentError;
    use crate::hooks::{Hook, HookContext, HookPhase, HookRegistry};
    use crate::message::{ContentBlock, Message, ToolCall};
    use crate::mock::MockProvider;
    use crate::provider::{CompletionResponse, StopReason};
    use crate::runtime::{AgentConfig, AgentSession};
    use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry};

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

        async fn invoke(
            &self,
            arguments: Value,
            _context: ToolContext,
        ) -> Result<ToolOutput, AgentError> {
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
                ..AgentConfig::default()
            });

            let result = session
                .run_turn(&provider, &tools, "what is 2 + 3?")
                .await
                .unwrap();
            assert_eq!(result.final_message.text_content(), "The answer is 5.");
            assert_eq!(result.stop_reason, StopReason::EndTurn);
            assert!(result.events.len() >= 11);
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

    struct DenyTool;

    #[async_trait]
    impl Tool for DenyTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "deny".into(),
                description: "Always deny".into(),
                input_schema: json!({ "type": "object" }),
            }
        }

        async fn check_permission(
            &self,
            _arguments: &Value,
            _context: &ToolContext,
        ) -> Result<PermissionDecision, AgentError> {
            Ok(PermissionDecision::Deny {
                reason: "policy blocked".into(),
            })
        }

        async fn invoke(
            &self,
            _arguments: Value,
            _context: ToolContext,
        ) -> Result<ToolOutput, AgentError> {
            Err(AgentError::ToolExecution {
                tool: "deny".into(),
                message: "should not run".into(),
            })
        }
    }

    #[test]
    fn permission_denial_returns_structured_tool_error() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let provider = MockProvider::new(vec![
                CompletionResponse {
                    message: Message {
                        role: crate::Role::Assistant,
                        blocks: vec![ContentBlock::ToolCall(ToolCall {
                            id: "call-1".into(),
                            name: "deny".into(),
                            arguments: json!({}),
                        })],
                    },
                    stop_reason: StopReason::ToolUse,
                },
                CompletionResponse {
                    message: Message::assistant("Denied."),
                    stop_reason: StopReason::EndTurn,
                },
            ]);

            let mut tools = ToolRegistry::new();
            tools.register(DenyTool);

            let mut session = AgentSession::new(AgentConfig::default());
            let result = session.run_turn(&provider, &tools, "try deny").await.unwrap();
            let tool_result_event = result
                .events
                .iter()
                .find(|event| matches!(event, crate::TurnEvent::ToolResult(_)))
                .unwrap();

            assert!(tool_result_event
                .text()
                .contains("\"kind\":\"permission_denied\""));
        });
    }

    struct EchoStopHook;

    #[async_trait]
    impl Hook for EchoStopHook {
        fn name(&self) -> &str {
            "echo-stop"
        }

        fn phase(&self) -> HookPhase {
            HookPhase::Stop
        }

        async fn run(
            &self,
            _context: &HookContext,
            _message: &Message,
        ) -> Result<Option<Message>, AgentError> {
            Ok(Some(Message::assistant("hook-ran")))
        }
    }

    #[test]
    fn run_turn_stream_emits_deltas_and_hooks() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut hooks = HookRegistry::new();
            hooks.register(EchoStopHook);

            let provider = MockProvider::new(vec![CompletionResponse {
                message: Message::assistant("hello"),
                stop_reason: StopReason::EndTurn,
            }]);
            let tools = ToolRegistry::new();
            let mut session = AgentSession::new(AgentConfig {
                hooks: Arc::new(hooks),
                ..AgentConfig::default()
            });

            let events = session
                .run_turn_stream(&provider, &tools, "hi")
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            assert!(events.iter().any(|event| {
                matches!(event, crate::TurnEvent::AssistantDelta { text } if text == "hello")
            }));
            assert!(events.iter().any(|event| {
                matches!(event, crate::TurnEvent::HookStarted { phase, .. } if *phase == "stop")
            }));
            assert_eq!(session.messages().last().unwrap().text_content(), "hook-ran");
        });
    }

    struct BigTool;

    #[async_trait]
    impl Tool for BigTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "big".into(),
                description: "Return a large result".into(),
                input_schema: json!({ "type": "object" }),
            }
        }

        async fn invoke(
            &self,
            _arguments: Value,
            _context: ToolContext,
        ) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput {
                content: json!({ "blob": "x".repeat(100) }),
            })
        }
    }

    #[test]
    fn tool_result_budget_compacts_large_output() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let provider = MockProvider::new(vec![
                CompletionResponse {
                    message: Message {
                        role: crate::Role::Assistant,
                        blocks: vec![ContentBlock::ToolCall(ToolCall {
                            id: "call-1".into(),
                            name: "big".into(),
                            arguments: json!({}),
                        })],
                    },
                    stop_reason: StopReason::ToolUse,
                },
                CompletionResponse {
                    message: Message::assistant("done"),
                    stop_reason: StopReason::EndTurn,
                },
            ]);
            let mut tools = ToolRegistry::new();
            tools.register(BigTool);
            let mut session = AgentSession::new(AgentConfig {
                max_tool_result_chars: 20,
                ..AgentConfig::default()
            });
            let result = session.run_turn(&provider, &tools, "run").await.unwrap();
            assert!(result.events.iter().any(|event| matches!(event, crate::TurnEvent::CompactionStarted { .. })));
            assert!(result.events.iter().any(|event| matches!(event, crate::TurnEvent::CompactionCompleted { .. })));
            let tool_result = result
                .events
                .iter()
                .find_map(|event| match event {
                    crate::TurnEvent::ToolResult(_) => Some(event),
                    _ => None,
                })
                .unwrap();
            assert!(tool_result.text().contains("truncated"));
        });
    }
}
