use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::sync::Arc;

use tiny_agent_core::AgentError;
use tiny_agent_core::MockProvider;
use tiny_agent_core::register_core_tools;
use tiny_agent_core::{AgentConfig, AgentSession, TurnEvent};
use tiny_agent_core::{ApprovalDecision, FixedDecisionHandler, SubagentTool, TokenBudget};
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

    fn aliases(&self) -> &'static [&'static str] {
        &["legacy_add"]
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let a = arguments["a"].as_i64().ok_or_else(|| AgentError::ToolExecution {
            tool: "add".into(),
            message: "missing integer `a`".into(),
        })?;
        let b = arguments["b"].as_i64().ok_or_else(|| AgentError::ToolExecution {
            tool: "add".into(),
            message: "missing integer `b`".into(),
        })?;

        Ok(ToolOutput { content: json!({ "sum": a + b }) })
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
            base_system_prompt: Some("You are a coding agent.".into()),
            max_iterations: 4,
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "what is 2 + 3?").await.unwrap();
        assert_eq!(result.final_message.text_content(), "The answer is 5.");
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert!(result.events.len() >= 11);
        assert_eq!(session.messages().len(), 5);
    });
}

#[test]
fn tool_calls_continue_even_when_stop_reason_is_end_turn() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 4, "b": 6 }),
                    })],
                },
                // Some providers/proxies get this wrong. The runtime should
                // follow the actual assistant content blocks.
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("The answer is 10."),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);

        let mut tools = ToolRegistry::new();
        tools.register(AddTool);
        let mut session = AgentSession::new(AgentConfig::default()).unwrap();

        let result = session.run_turn(&provider, &tools, "add").await.unwrap();
        assert_eq!(result.final_message.text_content(), "The answer is 10.");
        assert!(result.events.iter().any(|event| matches!(event, TurnEvent::ToolResult(_))));
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
        let mut session = AgentSession::new(AgentConfig::default()).unwrap();

        let result = session.run_turn(&provider, &tools, "try a tool").await.unwrap();
        let tool_result_event =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_)));

        assert!(tool_result_event.is_some());
        assert!(tool_result_event.unwrap().text().contains("tool not found: missing"));
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
        Ok(PermissionDecision::Deny { reason: "policy blocked".into() })
    }

    async fn invoke(
        &self,
        _arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        Err(AgentError::ToolExecution { tool: "deny".into(), message: "should not run".into() })
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

        let mut session = AgentSession::new(AgentConfig::default()).unwrap();
        let result = session.run_turn(&provider, &tools, "try deny").await.unwrap();
        let tool_result_event =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();

        assert!(tool_result_event.text().contains("\"kind\":\"permission_denied\""));
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
        let mut session =
            AgentSession::new(AgentConfig { hooks: Arc::new(hooks), ..AgentConfig::default() })
                .unwrap();

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
        assert_eq!(session.messages().last().unwrap().text_content(), "hook-ran");
    });
}

#[test]
fn stop_hook_does_not_hijack_final_message() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut hooks = HookRegistry::new();
        hooks.register(EchoStopHook);

        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("model answer"),
            stop_reason: StopReason::EndTurn,
            usage: None,
        }]);
        let tools = ToolRegistry::new();
        let mut session =
            AgentSession::new(AgentConfig { hooks: Arc::new(hooks), ..AgentConfig::default() })
                .unwrap();

        let result = session.run_turn(&provider, &tools, "hi").await.unwrap();
        // The last session message is the hook output, but the turn result
        // should still expose the model's own final answer.
        assert_eq!(session.messages().last().unwrap().text_content(), "hook-ran");
        assert_eq!(result.final_message.text_content(), "model answer");
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
        Ok(ToolOutput { content: json!({ "blob": "x".repeat(100) }) })
    }
}

struct ProgressTool;

#[async_trait]
impl Tool for ProgressTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "progress".into(),
            description: "Emit progress before completing".into(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn invoke(
        &self,
        _arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        if let Some(tx) = &context.progress {
            let _ = tx.send(tiny_agent_core::ToolProgress {
                tool_call_id: None,
                message: "halfway".into(),
                data: None,
            });
        }
        Ok(ToolOutput::json(json!({ "done": true })))
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
        let mut session =
            AgentSession::new(AgentConfig { max_tool_result_chars: 20, ..AgentConfig::default() })
                .unwrap();
        let result = session.run_turn(&provider, &tools, "run").await.unwrap();
        assert!(
            result.events.iter().any(|event| matches!(event, TurnEvent::CompactionStarted { .. }))
        );
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event, TurnEvent::CompactionCompleted { .. }))
        );
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
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

        let messages =
            vec![Message::system("sys"), Message::user("hello"), Message::assistant("world")];
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
            base_system_prompt: Some("sys".into()),
            storage: Some(storage.clone()),
            ..AgentConfig::default()
        })
        .unwrap();

        session.run_turn(&provider, &tools, "hello").await.unwrap();
        assert_eq!(session.messages().len(), 3); // sys + user + assistant

        let session_id = session.session_id().to_string();
        let resumed = AgentSession::resume(
            session_id,
            AgentConfig {
                base_system_prompt: Some("sys".into()),
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
        })
        .unwrap();
        let result = session.run_turn(&provider, &tools, "add").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
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
        })
        .unwrap();
        let result = session.run_turn(&provider, &tools, "add").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("\"sum\":3"));
    });
}

#[test]
fn permission_engine_matches_tool_aliases_with_last_rule_wins() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::deny_tool("*"));
        engine.add_rule(PermissionRule::allow_tool("legacy_add"));

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
        })
        .unwrap();
        let result = session.run_turn(&provider, &tools, "add").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
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
            compaction: Some(Arc::new(SummaryCompaction { max_tokens: 10, keep_recent: 2 })),
            max_tool_result_chars: usize::MAX,
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "run big").await.unwrap();
        assert!(
            result.events.iter().any(|event| matches!(event, TurnEvent::CompactionStarted { .. }))
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
        })
        .unwrap();

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
                        arguments: json!({ "file_path": "sample.txt" }),
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
        let mut session =
            AgentSession::new(AgentConfig { cwd: dir.clone(), ..AgentConfig::default() }).unwrap();

        let result = session.run_turn(&provider, &tools, "read").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("1: alpha"));
        assert!(tool_result.text().contains("2: beta"));

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn file_read_rejects_symlink_escape() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir =
            std::env::temp_dir().join(format!("tiny-agent-symlink-test-{}", std::process::id()));
        let outside =
            std::env::temp_dir().join(format!("tiny-agent-symlink-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "super-secret").unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), dir.join("link.txt")).unwrap();

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "Read".into(),
                        arguments: json!({ "file_path": "link.txt" }),
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
        let mut session =
            AgentSession::new(AgentConfig { cwd: dir.clone(), ..AgentConfig::default() }).unwrap();

        let result = session.run_turn(&provider, &tools, "read symlink").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(
            tool_result.text().contains("permission_denied")
                || tool_result.text().contains("escapes cwd"),
            "{}",
            tool_result.text()
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    });
}

#[test]
fn core_tools_expose_claude_names_and_accept_legacy_aliases() {
    let mut tools = ToolRegistry::new();
    register_core_tools(&mut tools);

    let names =
        tools.definitions().into_iter().map(|definition| definition.name).collect::<Vec<_>>();
    assert!(names.contains(&"Bash".to_string()));
    assert!(names.contains(&"Read".to_string()));
    assert!(names.contains(&"Edit".to_string()));
    assert!(names.contains(&"Write".to_string()));
    assert!(!names.contains(&"shell".to_string()));
    assert!(tools.get("shell").is_ok());
    assert!(tools.get("file_read").is_ok());
}

#[test]
fn edit_requires_prior_full_read() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir = std::env::temp_dir()
            .join(format!("tiny-agent-edit-read-required-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sample.txt"), "alpha\nbeta\n").unwrap();

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "Edit".into(),
                        arguments: json!({
                            "file_path": "sample.txt",
                            "old_string": "beta",
                            "new_string": "gamma"
                        }),
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
            permission_engine: Some({
                let mut engine = PermissionEngine::new();
                engine.add_rule(PermissionRule::allow_tool("Edit"));
                engine
            }),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "edit").await.unwrap();
        let tool_result = result
            .events
            .iter()
            .find_map(|event| match event {
                TurnEvent::ToolResult(_) => Some(event.text()),
                _ => None,
            })
            .unwrap();
        assert!(tool_result.contains("File has not been read yet"));
        assert_eq!(std::fs::read_to_string(dir.join("sample.txt")).unwrap(), "alpha\nbeta\n");

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn edit_rejects_stale_file_after_read() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir =
            std::env::temp_dir().join(format!("tiny-agent-edit-stale-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sample.txt");
        std::fs::write(&file, "alpha\nbeta\n").unwrap();

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "Read".into(),
                        arguments: json!({ "file_path": "sample.txt" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-2".into(),
                        name: "Edit".into(),
                        arguments: json!({
                            "file_path": "sample.txt",
                            "old_string": "beta",
                            "new_string": "gamma"
                        }),
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
            permission_engine: Some({
                let mut engine = PermissionEngine::new();
                engine.add_rule(PermissionRule::allow_tool("Edit"));
                engine
            }),
            ..AgentConfig::default()
        })
        .unwrap();

        let mut stream = Box::pin(session.run_turn_stream(&provider, &tools, "read then edit"));
        let mut saw_read_result = false;
        let mut saw_stale_error = false;
        while let Some(event) = stream.next().await {
            let event = event.unwrap();
            if matches!(event, TurnEvent::ToolResult(_)) && !saw_read_result {
                saw_read_result = true;
                std::thread::sleep(std::time::Duration::from_millis(2));
                std::fs::write(&file, "alpha\nuser change\n").unwrap();
            } else if let TurnEvent::ToolResult(message) = event {
                saw_stale_error = message.tool_results_iter().any(|result| {
                    result.content.to_string().contains("File has been modified since read")
                });
            }
        }

        assert!(saw_stale_error);
        assert_eq!(std::fs::read_to_string(file).unwrap(), "alpha\nuser change\n");

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
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "shell").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("allowed"), "{}", tool_result.text());
    });
}

#[test]
fn shell_requires_approval_by_default() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "shell".into(),
                        // Use a command that the analyzer classifies as needing
                        // review (output redirect) now that safe commands are
                        // auto-approved.
                        arguments: json!({ "command": "echo hello > file.txt" }),
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
        let mut session = AgentSession::new(AgentConfig::default()).unwrap();

        let result = session.run_turn(&provider, &tools, "shell").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("permission_required"), "{}", tool_result.text());
    });
}

#[test]
fn tool_progress_streams_before_tool_result() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "progress".into(),
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
        tools.register(ProgressTool);
        let mut session = AgentSession::new(AgentConfig::default()).unwrap();

        let events = session
            .run_turn_stream(&provider, &tools, "go")
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let progress_idx = events
            .iter()
            .position(|event| matches!(event, TurnEvent::ToolProgress { message, .. } if message == "halfway"))
            .unwrap();
        let result_idx = events
            .iter()
            .position(|event| matches!(event, TurnEvent::ToolResult(_)))
            .unwrap();
        assert!(progress_idx < result_idx);
    });
}

#[test]
fn token_budget_triggers_auto_compaction() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message::assistant("summary"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: Some(tiny_agent_core::TokenUsage { input_tokens: 10, output_tokens: 2 }),
            },
        ]);
        let tools = ToolRegistry::new();
        let mut session = AgentSession::new(AgentConfig {
            base_system_prompt: Some("sys".into()),
            compaction: Some(Arc::new(SummaryCompaction { max_tokens: 50, keep_recent: 0 })),
            token_budget: Some(TokenBudget { max_tokens: 1_000, compact_at_tokens: 10 }),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "x".repeat(200)).await.unwrap();
        assert!(result.events.iter().any(|event| {
            matches!(event, TurnEvent::CompactionStarted { reason } if reason == "token_budget")
        }));
        assert!(result.events.iter().any(|event| {
            matches!(event, TurnEvent::ProviderUsage { input_tokens: 10, output_tokens: 2 })
        }));
    });
}

#[test]
fn subagent_tool_runs_in_process_agent() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let outer_provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "subagent".into(),
                        arguments: json!({ "prompt": "solve inside" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("outer done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let inner_provider = Arc::new(MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("inner answer"),
            stop_reason: StopReason::EndTurn,
            usage: None,
        }]));
        let mut tools = ToolRegistry::new();
        tools.register(SubagentTool::new(
            inner_provider,
            ToolRegistry::new(),
            AgentConfig::default(),
        ));
        let mut session = AgentSession::new(AgentConfig {
            approval_handler: Some(Arc::new(FixedDecisionHandler {
                decision: ApprovalDecision::Allow,
            })),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&outer_provider, &tools, "delegate").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("inner answer"));
    });
}

#[test]
fn thinking_blocks_are_separate_from_final_text() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message {
                role: tiny_agent_core::Role::Assistant,
                blocks: vec![
                    ContentBlock::Thinking(tiny_agent_core::ThinkingBlock {
                        text: "I need to reason about this.".into(),
                        signature: None,
                        is_redacted: false,
                    }),
                    ContentBlock::Text(tiny_agent_core::TextBlock {
                        text: "The answer is 7.".into(),
                    }),
                ],
            },
            stop_reason: StopReason::EndTurn,
            usage: None,
        }]);

        let tools = ToolRegistry::new();
        let mut session = AgentSession::new(AgentConfig::default()).unwrap();

        let result = session.run_turn(&provider, &tools, "what is 3 + 4?").await.unwrap();
        assert_eq!(result.final_message.text_content(), "The answer is 7.");
        assert_eq!(result.final_message.thinking_content(), "I need to reason about this.");

        // The streaming turn loop should emit at least one thinking delta.
        assert!(result.events.iter().any(|event| matches!(event, TurnEvent::ThinkingDelta { .. })));

        // text_content should not leak into the final answer.
        assert!(!result.final_message.text_content().contains("reason"));
    });
}

#[test]
fn schema_validation_rejects_invalid_tool_arguments() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": "not an integer", "b": 3 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("Schema error."),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);

        let mut tools = ToolRegistry::new();
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig::default()).unwrap();
        let result = session.run_turn(&provider, &tools, "add wrong types").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("validation_error"));
        assert!(tool_result.text().contains("schema validation"));
    });
}

#[test]
fn schema_validation_can_be_disabled() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": "not an integer", "b": 3 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("Done."),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);

        let mut tools = ToolRegistry::new();
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig {
            auto_validate_schema: false,
            ..AgentConfig::default()
        })
        .unwrap();
        let result = session.run_turn(&provider, &tools, "add wrong types").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        // The tool's own invoke fails because it expects an integer, not schema validation.
        assert!(tool_result.text().contains("missing integer `a`"));
    });
}

#[test]
fn approval_handler_allows_asked_tool_call() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::ask_tool("add"));

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 2, "b": 3 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("Approved."),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);

        let mut tools = ToolRegistry::new();
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            approval_handler: Some(Arc::new(tiny_agent_core::FixedDecisionHandler {
                decision: tiny_agent_core::ApprovalDecision::Allow,
            })),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "add").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("\"sum\":5"));
        assert!(
            result.events.iter().any(|event| matches!(event, TurnEvent::ApprovalRequested { .. }))
        );
        assert!(
            result.events.iter().any(|event| matches!(event, TurnEvent::ApprovalResolved { .. }))
        );
    });
}

#[test]
fn approval_handler_denies_asked_tool_call() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::ask_tool("add"));

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 2, "b": 3 }),
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
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            approval_handler: Some(Arc::new(tiny_agent_core::FixedDecisionHandler {
                decision: tiny_agent_core::ApprovalDecision::Deny { reason: "not today".into() },
            })),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "add").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("permission_denied"));
    });
}

#[test]
fn approval_handler_modifies_asked_tool_call() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::ask_tool("add"));

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: tiny_agent_core::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 2, "b": 3 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("Modified."),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);

        let mut tools = ToolRegistry::new();
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            approval_handler: Some(Arc::new(tiny_agent_core::FixedDecisionHandler {
                decision: tiny_agent_core::ApprovalDecision::Modify {
                    arguments: json!({ "a": 10, "b": 5 }),
                },
            })),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "add").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("\"sum\":15"));
    });
}

#[tokio::test]
async fn skill_tool_invokes_and_returns_prompt() {
    use std::sync::Arc;
    use tiny_agent_core::skills::{Skill, SkillArg, SkillRegistry, SkillSource};
    use tiny_agent_core::tool::{Tool, ToolContext};
    use tiny_agent_core::tools::SkillTool;

    let mut reg = SkillRegistry::new();
    reg.register(Skill {
        name: "greet".into(),
        description: "Greets the user".into(),
        when_to_use: None,
        prompt: "Say hello to {{args}}!".into(),
        arguments: vec![SkillArg {
            name: "name".into(),
            description: "Who to greet".into(),
            required: true,
        }],
        body: String::new(),
        source: SkillSource::Bundled,
    });

    let tool = SkillTool::new(Arc::new(reg));
    let def = tool.definition();
    assert_eq!(def.name, "Skill");

    let ctx = ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        cwd: std::env::current_dir().unwrap(),
        env: Default::default(),
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    };

    let result =
        tool.invoke(serde_json::json!({"skill": "greet", "args": "World"}), ctx).await.unwrap();

    let content = result.content;
    assert!(content["text"].as_str().unwrap().contains("Say hello to World"));
    assert_eq!(content["skill_name"].as_str().unwrap(), "greet");
}

#[tokio::test]
async fn skill_loader_parses_valid_markdown() {
    use tiny_agent_core::skills::{SkillLoader, SkillSource};

    let dir = tempfile::tempdir().unwrap();
    let skill_content = r#"---
name: test-skill
description: A test skill
whenToUse: When testing
prompt: "You are a test skill. Args: {{args}}"
arguments:
  - name: args
    description: Optional args
    required: false
---
This is the body text.
"#;
    std::fs::write(dir.path().join("test-skill.md"), skill_content).unwrap();

    let skills = SkillLoader::load_from_dir(dir.path()).unwrap();
    assert_eq!(skills.len(), 1);
    let s = &skills[0];
    assert_eq!(s.name, "test-skill");
    assert_eq!(s.description, "A test skill");
    assert_eq!(s.when_to_use, Some("When testing".into()));
    assert!(s.prompt.contains("You are a test skill"));
    assert!(s.body.contains("This is the body text"));
    assert_eq!(s.arguments.len(), 1);
    assert_eq!(s.arguments[0].name, "args");
    assert_eq!(s.source, SkillSource::Project);
}

#[test]
fn skill_loader_empty_directory_returns_empty() {
    use tiny_agent_core::skills::SkillLoader;

    let dir = tempfile::tempdir().unwrap();
    let skills = SkillLoader::load_from_dir(dir.path()).unwrap();
    assert!(skills.is_empty());
}

#[test]
fn skill_loader_skips_non_md_files() {
    use tiny_agent_core::skills::SkillLoader;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "not a skill").unwrap();
    let skills = SkillLoader::load_from_dir(dir.path()).unwrap();
    assert!(skills.is_empty());
}

#[test]
fn skill_loader_skips_malformed_yaml() {
    use tiny_agent_core::skills::SkillLoader;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.md"), "---\nname: bad\nnot: valid:\n---\nbody").unwrap();
    let skills = SkillLoader::load_from_dir(dir.path()).unwrap();
    // Malformed YAML should be gracefully skipped
    assert!(skills.is_empty());
}

#[test]
fn skill_registry_override_priority() {
    use tiny_agent_core::skills::{Skill, SkillRegistry, SkillSource};

    let mut reg = SkillRegistry::new();
    reg.register(Skill {
        name: "my-skill".into(),
        description: "bundled desc".into(),
        when_to_use: Some("for testing".into()),
        prompt: "bundled prompt".into(),
        arguments: vec![],
        body: String::new(),
        source: SkillSource::Bundled,
    });
    reg.register(Skill {
        name: "my-skill".into(),
        description: "user desc".into(),
        when_to_use: Some("for testing".into()),
        prompt: "user prompt".into(),
        arguments: vec![],
        body: String::new(),
        source: SkillSource::User,
    });
    let skill = reg.get("my-skill").unwrap();
    assert_eq!(skill.prompt, "user prompt");
}

#[test]
fn skill_registry_render_for_prompt() {
    use tiny_agent_core::skills::{Skill, SkillArg, SkillRegistry, SkillSource};

    let mut reg = SkillRegistry::new();
    reg.register(Skill {
        name: "verify".into(),
        description: "Verify code changes".into(),
        when_to_use: Some("Before committing".into()),
        prompt: "Verify prompt".into(),
        arguments: vec![SkillArg {
            name: "target".into(),
            description: "What to verify".into(),
            required: false,
        }],
        body: String::new(),
        source: SkillSource::Bundled,
    });
    let rendered = reg.render_for_prompt();
    assert!(rendered.contains("verify"));
    assert!(rendered.contains("Verify code changes"));
    assert!(rendered.contains("Before committing"));
}

#[test]
fn skill_registry_empty_renders_empty_string() {
    use tiny_agent_core::skills::SkillRegistry;
    let reg = SkillRegistry::new();
    assert_eq!(reg.render_for_prompt(), "");
}

#[test]
fn skill_registry_get_missing_returns_none() {
    use tiny_agent_core::skills::SkillRegistry;
    let reg = SkillRegistry::new();
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn bundled_skills_load_successfully() {
    use tiny_agent_core::skills::SkillLoader;
    let skills = SkillLoader::load_bundled_skills();
    assert!(skills.len() >= 5, "expected >=5 bundled skills, got {}", skills.len());
    for s in &skills {
        assert!(!s.name.is_empty(), "skill has empty name");
        assert!(!s.description.is_empty(), "skill '{}' has empty description", s.name);
        assert!(!s.prompt.is_empty(), "skill '{}' has empty prompt", s.name);
        assert_eq!(s.source, tiny_agent_core::skills::SkillSource::Bundled);
    }
}

#[tokio::test]
async fn prompt_assembly_caches_static_sections() {
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tiny_agent_core::prompt::{PromptAssembly, PromptSection, PromptStability};

    static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct StaticSection;
    #[async_trait]
    impl PromptSection for StaticSection {
        fn name(&self) -> &str {
            "static_test"
        }
        fn stability(&self) -> PromptStability {
            PromptStability::Static
        }
        async fn render(&self, _ctx: &()) -> String {
            CALL_COUNT.fetch_add(1, Ordering::Relaxed);
            "static content".into()
        }
    }

    struct DynamicSection;
    #[async_trait]
    impl PromptSection for DynamicSection {
        fn name(&self) -> &str {
            "dynamic_test"
        }
        fn stability(&self) -> PromptStability {
            PromptStability::Dynamic
        }
        async fn render(&self, _ctx: &()) -> String {
            CALL_COUNT.fetch_add(1, Ordering::Relaxed);
            "dynamic content".into()
        }
    }

    let mut assembly = PromptAssembly::new();
    assembly.add_static(StaticSection);
    assembly.add_dynamic(DynamicSection);

    let result1 = assembly.build().await;
    assert!(result1.contains("static content"));
    assert!(result1.contains("dynamic content"));

    CALL_COUNT.store(0, Ordering::Relaxed);
    let result2 = assembly.build().await;
    // Static cached: only dynamic re-renders = 1 call
    let calls = CALL_COUNT.load(Ordering::Relaxed);
    assert_eq!(calls, 1, "static section should be cached, only dynamic re-rendered");
    assert!(result2.contains("static content"));
}

#[tokio::test]
async fn builtin_prompt_sections_render_without_error() {
    use tiny_agent_core::prompt::PromptAssembly;
    use tiny_agent_core::prompt::builtins::*;
    use tiny_agent_core::tool::ToolRegistry;

    let mut assembly = PromptAssembly::new();
    assembly.add_static(IdentitySection::new(Some("Be helpful.".into())));
    assembly.add_static(ToolsSection::new(std::sync::Arc::new(ToolRegistry::new())));
    assembly.add_dynamic(DateSection);
    assembly.add_dynamic(CwdSection::new(std::env::current_dir().unwrap()));
    assembly.add_dynamic(GitStatusSection);

    let result = assembly.build().await;
    assert!(result.contains("tiny-agent"));
    assert!(result.contains("Today's date"));
    assert!(result.contains("Working directory"));
}

#[test]
fn prompt_assembly_integration_with_session() {
    use async_trait::async_trait;
    use tiny_agent_core::prompt::{PromptAssembly, PromptSection, PromptStability};

    struct TestSection;
    #[async_trait]
    impl PromptSection for TestSection {
        fn name(&self) -> &str {
            "test"
        }
        fn stability(&self) -> PromptStability {
            PromptStability::Static
        }
        async fn render(&self, _ctx: &()) -> String {
            "TEST_SECTION_CONTENT".into()
        }
    }

    let mut assembly = PromptAssembly::new();
    assembly.add_static(TestSection);

    let config = AgentConfig {
        prompt_assembly: Some(std::sync::Arc::new(assembly)),
        ..AgentConfig::default()
    };

    let session = AgentSession::new(config).unwrap();
    assert!(session.messages().is_empty()); // assembly renders at turn time
}

#[tokio::test]
async fn memory_write_and_read_tools_roundtrip() {
    use std::sync::{Arc, Mutex};
    use tiny_agent_core::memory::{MemoryReadTool, MemoryStore, MemoryWriteTool};
    use tiny_agent_core::tool::{Tool, ToolContext};

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(MemoryStore::new(dir.path().to_path_buf())));
    let write_tool = MemoryWriteTool::new(store.clone());
    let read_tool = MemoryReadTool::new(store.clone());

    let ctx = ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        cwd: std::env::current_dir().unwrap(),
        env: Default::default(),
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    };

    // Write
    write_tool
        .invoke(
            serde_json::json!({
                "name": "test-memory",
                "description": "A test memory entry",
                "category": "fact",
                "body": "This is the body content.",
                "tags": ["test", "example"]
            }),
            ctx.clone(),
        )
        .await
        .unwrap();

    // Read
    let result =
        read_tool.invoke(serde_json::json!({"name": "test-memory"}), ctx.clone()).await.unwrap();
    let content = result.content;
    assert_eq!(content["name"].as_str().unwrap(), "test-memory");
    assert_eq!(content["body"].as_str().unwrap(), "This is the body content.");
    assert!(content["tags"].as_array().unwrap().iter().any(|t| t.as_str() == Some("test")));
}

#[tokio::test]
async fn memory_section_renders_top_entries() {
    use std::sync::{Arc, Mutex};
    use tiny_agent_core::memory::{MemoryCategory, MemoryEntry, MemoryStatus, MemoryStore};
    use tiny_agent_core::prompt::PromptSection;
    use tiny_agent_core::prompt::builtins::MemorySection;

    let dir = tempfile::tempdir().unwrap();
    let mut store = MemoryStore::new(dir.path().to_path_buf());

    let entry = MemoryEntry {
        name: "test-fact".into(),
        description: "A test fact".into(),
        category: MemoryCategory::Fact,
        tags: vec!["test".into()],
        created: "2026-06-18".into(),
        updated: "2026-06-18".into(),
        status: MemoryStatus::Working,
        times_used: 5,
        confidence: None,
        related: vec![],
        source_session: None,
        body: "This is a test memory body.".into(),
    };
    store.write(entry).unwrap();

    let section = MemorySection::new(Arc::new(Mutex::new(store)));
    let rendered = section.render(&()).await;
    assert!(rendered.contains("Relevant Memories"));
    assert!(rendered.contains("test-fact"));
    assert!(rendered.contains("A test fact"));
}

#[tokio::test]
async fn memory_section_empty_when_no_memories() {
    use std::sync::{Arc, Mutex};
    use tiny_agent_core::memory::MemoryStore;
    use tiny_agent_core::prompt::PromptSection;
    use tiny_agent_core::prompt::builtins::MemorySection;

    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new(dir.path().to_path_buf());
    let section = MemorySection::new(Arc::new(Mutex::new(store)));
    let rendered = section.render(&()).await;
    assert!(rendered.is_empty());
}

#[tokio::test]
async fn profile_section_renders_profiles() {
    use std::sync::Arc;
    use tiny_agent_core::memory::ProfileManager;
    use tiny_agent_core::prompt::PromptSection;
    use tiny_agent_core::prompt::builtins::ProfileSection;

    let dir = tempfile::tempdir().unwrap();
    let mgr =
        Arc::new(ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap());
    mgr.set_user_profile("Test user profile content").unwrap();
    mgr.set_project_profile("Test project profile content").unwrap();

    let section = ProfileSection::new(mgr);
    let rendered = section.render(&()).await;
    assert!(rendered.contains("User Profile"));
    assert!(rendered.contains("Test user profile content"));
    assert!(rendered.contains("Project Profile"));
    assert!(rendered.contains("Test project profile content"));
}

#[tokio::test]
async fn profile_section_rerenders_when_profiles_change() {
    use std::sync::Arc;
    use tiny_agent_core::memory::ProfileManager;
    use tiny_agent_core::prompt::PromptAssembly;
    use tiny_agent_core::prompt::builtins::ProfileSection;

    let dir = tempfile::tempdir().unwrap();
    let mgr =
        Arc::new(ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap());
    mgr.set_user_profile("Before").unwrap();

    let mut assembly = PromptAssembly::new();
    assembly.add_dynamic(ProfileSection::new(mgr.clone()));

    let first = assembly.build().await;
    assert!(first.contains("Before"));

    mgr.set_user_profile("After").unwrap();
    let second = assembly.build().await;
    assert!(second.contains("After"));
    assert!(!second.contains("Before"));
}
