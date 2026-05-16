use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::sync::Arc;

use tiny_agent_core::AgentError;
use tiny_agent_core::MockProvider;
use tiny_agent_core::register_core_tools;
use tiny_agent_core::{AgentConfig, AgentSession, TurnEvent};
use tiny_agent_core::{CompletionResponse, StopReason};
use tiny_agent_core::{ContentBlock, Message, ToolCall};
use tiny_agent_core::{Hook, HookContext, HookPhase, HookRegistry};
use tiny_agent_core::{JsonlStorage, PermissionEngine, PermissionRule, Storage, SummaryCompaction};
use tiny_agent_core::{
    PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry,
};

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
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![
                        ContentBlock::Text(tiny_agent_core::TextBlock {
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
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("The answer is 5."),
                stop_reason: StopReason::EndTurn,
                usage: None,
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
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "missing".into(),
                        arguments: json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("I could not run that tool."),
                stop_reason: StopReason::EndTurn,
                usage: None,
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
            .find(|event| matches!(event, TurnEvent::ToolResult(_)));

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
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "deny".into(),
                        arguments: json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("Denied."),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);

        let mut tools = ToolRegistry::new();
        tools.register(DenyTool);

        let mut session = AgentSession::new(AgentConfig::default());
        let result = session
            .run_turn(&provider, &tools, "try deny")
            .await
            .unwrap();
        let tool_result_event = result
            .events
            .iter()
            .find(|event| matches!(event, TurnEvent::ToolResult(_)))
            .unwrap();

        assert!(
            tool_result_event
                .text()
                .contains("\"kind\":\"permission_denied\"")
        );
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
            usage: None,
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
            matches!(event, TurnEvent::AssistantDelta { text } if text == "hello")
        }));
        assert!(events.iter().any(|event| {
            matches!(event, TurnEvent::HookStarted { phase, .. } if *phase == "stop")
        }));
        assert_eq!(
            session.messages().last().unwrap().text_content(),
            "hook-ran"
        );
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
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "big".into(),
                        arguments: json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        tools.register(BigTool);
        let mut session = AgentSession::new(AgentConfig {
            max_tool_result_chars: 20,
            ..AgentConfig::default()
        });
        let result = session.run_turn(&provider, &tools, "run").await.unwrap();
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event, TurnEvent::CompactionStarted { .. }))
        );
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event, TurnEvent::CompactionCompleted { .. }))
        );
        let tool_result = result
            .events
            .iter()
            .find_map(|event| match event {
                TurnEvent::ToolResult(_) => Some(event),
                _ => None,
            })
            .unwrap();
        assert!(tool_result.text().contains("truncated"));
    });
}

#[test]
fn jsonl_storage_roundtrips_messages() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir =
            std::env::temp_dir().join(format!("tiny-agent-roundtrip-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = JsonlStorage::new(&dir).unwrap();

        let messages = vec![
            Message::system("sys"),
            Message::user("hello"),
            Message::assistant("world"),
        ];
        storage.append("s1", &messages).await.unwrap();

        let loaded = storage.load("s1").await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].text_content(), "sys");
        assert_eq!(loaded[1].text_content(), "hello");
        assert_eq!(loaded[2].text_content(), "world");

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn session_save_and_resume_works() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir =
            std::env::temp_dir().join(format!("tiny-agent-resume-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = Arc::new(JsonlStorage::new(&dir).unwrap());

        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("hi there"),
            stop_reason: StopReason::EndTurn,
            usage: None,
        }]);
        let tools = ToolRegistry::new();
        let mut session = AgentSession::new(AgentConfig {
            system_prompt: Some("sys".into()),
            storage: Some(storage.clone()),
            ..AgentConfig::default()
        });

        session.run_turn(&provider, &tools, "hello").await.unwrap();
        assert_eq!(session.messages().len(), 3); // sys + user + assistant

        let session_id = session.session_id().to_string();
        let resumed = AgentSession::resume(
            session_id,
            AgentConfig {
                system_prompt: Some("sys".into()),
                storage: Some(storage.clone()),
                ..AgentConfig::default()
            },
            storage.clone(),
        )
        .await
        .unwrap();

        assert_eq!(resumed.messages().len(), 3);
        assert_eq!(resumed.messages()[0].text_content(), "sys");
        assert_eq!(resumed.messages()[1].text_content(), "hello");
        assert_eq!(resumed.messages()[2].text_content(), "hi there");

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn permission_engine_denies_tool() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::deny_tool("add"));

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 1, "b": 2 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            ..AgentConfig::default()
        });
        let result = session.run_turn(&provider, &tools, "add").await.unwrap();
        let tool_result = result
            .events
            .iter()
            .find_map(|event| match event {
                TurnEvent::ToolResult(_) => Some(event),
                _ => None,
            })
            .unwrap();
        assert!(tool_result.text().contains("permission_denied"));
        assert!(tool_result.text().contains("permission rule"));
    });
}

#[test]
fn permission_engine_allows_tool() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::allow_tool("add"));

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 1, "b": 2 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            ..AgentConfig::default()
        });
        let result = session.run_turn(&provider, &tools, "add").await.unwrap();
        let tool_result = result
            .events
            .iter()
            .find_map(|event| match event {
                TurnEvent::ToolResult(_) => Some(event),
                _ => None,
            })
            .unwrap();
        assert!(tool_result.text().contains("\"sum\":3"));
    });
}

#[test]
fn summary_compaction_triggers_when_over_budget() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        // Provider responses: tool_use, summary, final_end_turn
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "big".into(),
                        arguments: json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("summary result"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        tools.register(BigTool);

        let mut session = AgentSession::new(AgentConfig {
            compaction: Some(Arc::new(SummaryCompaction {
                max_tokens: 10,
                keep_recent: 2,
            })),
            max_tool_result_chars: usize::MAX,
            ..AgentConfig::default()
        });

        let result = session
            .run_turn(&provider, &tools, "run big")
            .await
            .unwrap();
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event, TurnEvent::CompactionStarted { .. }))
        );
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event, TurnEvent::CompactionCompleted { .. }))
        );
    });
}

#[test]
fn session_save_replaces_snapshot_without_duplicates() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir =
            std::env::temp_dir().join(format!("tiny-agent-snapshot-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = Arc::new(JsonlStorage::new(&dir).unwrap());

        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("first"),
            stop_reason: StopReason::EndTurn,
            usage: None,
        }]);
        let tools = ToolRegistry::new();
        let mut session = AgentSession::new(AgentConfig {
            storage: Some(storage.clone()),
            ..AgentConfig::default()
        });

        session.run_turn(&provider, &tools, "hello").await.unwrap();
        session.save().await.unwrap();
        session.save().await.unwrap();

        let loaded = storage.load(session.session_id()).await.unwrap();
        assert_eq!(loaded.len(), session.messages().len());

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn builtin_file_read_tool_returns_file_contents() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir =
            std::env::temp_dir().join(format!("tiny-agent-file-read-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sample.txt"), "alpha\nbeta\n").unwrap();

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "file_read".into(),
                        arguments: json!({ "path": "sample.txt" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        register_core_tools(&mut tools);
        let mut session = AgentSession::new(AgentConfig {
            cwd: dir.clone(),
            ..AgentConfig::default()
        });

        let result = session.run_turn(&provider, &tools, "read").await.unwrap();
        let tool_result = result
            .events
            .iter()
            .find_map(|event| match event {
                TurnEvent::ToolResult(_) => Some(event),
                _ => None,
            })
            .unwrap();
        assert!(tool_result.text().contains("1: alpha"));
        assert!(tool_result.text().contains("2: beta"));

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn permission_engine_allows_shell_by_command_prefix() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::allow_tool("shell").command_prefix("echo"));

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "shell".into(),
                        arguments: json!({ "command": "echo allowed" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        register_core_tools(&mut tools);
        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            ..AgentConfig::default()
        });

        let result = session.run_turn(&provider, &tools, "shell").await.unwrap();
        let tool_result = result
            .events
            .iter()
            .find_map(|event| match event {
                TurnEvent::ToolResult(_) => Some(event),
                _ => None,
            })
            .unwrap();
        assert!(
            tool_result.text().contains("allowed"),
            "{}",
            tool_result.text()
        );
    });
}
