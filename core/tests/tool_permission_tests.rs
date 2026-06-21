mod common;

use futures_util::StreamExt;
use serde_json::json;
use std::sync::Arc;

use common::*;
use telos_agent::*;

#[test]
fn permission_engine_denies_tool() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::deny_tool("add"));

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 1, "b": 2 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
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
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 1, "b": 2 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
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
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 1, "b": 2 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
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

#[cfg(unix)]
#[test]
fn permission_engine_allows_shell_by_command_prefix() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::allow_tool("shell").command_prefix("echo"));

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "shell".into(),
                        arguments: json!({ "command": "echo allowed" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        register_core_tools_with_shell(&mut tools, DefaultShell::Bash);
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
fn permission_engine_allows_powershell_by_command_prefix() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::allow_tool("PowerShell").command_prefix("Get-Process"));

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "PowerShell".into(),
                        arguments: json!({ "command": "Get-Process -Name pwsh" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        register_core_tools_with_shell(&mut tools, DefaultShell::PowerShell);
        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "powershell").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(!tool_result.text().contains("permission_required"), "{}", tool_result.text());
        assert!(!tool_result.text().contains("tool_not_found"), "{}", tool_result.text());
    });
}

#[test]
fn shell_requires_approval_by_default() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
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
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
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

#[cfg(unix)]
#[test]
fn approval_modify_reruns_shell_safety_checks() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker.txt");
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "shell".into(),
                        arguments: json!({ "command": "echo original > marker.txt" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        register_core_tools(&mut tools);
        let mut session = AgentSession::new(AgentConfig {
            cwd: dir.path().to_path_buf(),
            approval_handler: Some(Arc::new(telos_agent::FixedDecisionHandler {
                decision: telos_agent::ApprovalDecision::Modify {
                    arguments: json!({ "command": "echo modified > marker.txt; rm -rf /" }),
                },
            })),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "shell").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("permission_required"), "{}", tool_result.text());
        assert!(!marker.exists());
    });
}

#[test]
fn approval_modify_reruns_powershell_safety_checks() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker.txt");
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "shell".into(),
                        arguments: json!({ "command": "Write-Output original > marker.txt" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);
        let mut tools = ToolRegistry::new();
        register_core_tools_with_shell(&mut tools, DefaultShell::PowerShell);
        let mut session = AgentSession::new(AgentConfig {
            cwd: dir.path().to_path_buf(),
            approval_handler: Some(Arc::new(telos_agent::FixedDecisionHandler {
                decision: telos_agent::ApprovalDecision::Modify {
                    arguments: json!({ "command": "Set-Content -Path marker.txt -Value modified" }),
                },
            })),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "powershell").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("permission_required"), "{}", tool_result.text());
        assert!(!marker.exists());
    });
}

#[test]
fn tool_progress_streams_before_tool_result() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "progress".into(),
                        arguments: json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
        model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
        model: None,
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
        assert!(events.iter().any(|event| {
            matches!(
                event,
                TurnEvent::ToolProgress {
                    tool_call_id: Some(tool_call_id),
                    message,
                    ..
                } if tool_call_id == "call-1" && message == "halfway"
            )
        }));
        let result_idx = events
            .iter()
            .position(|event| matches!(event, TurnEvent::ToolResult(_)))
            .unwrap();
        assert!(progress_idx < result_idx);
    });
}

#[test]
fn schema_validation_rejects_invalid_tool_arguments() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": "not an integer", "b": 3 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("Schema error."),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
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
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": "not an integer", "b": 3 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("Done."),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
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
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 2, "b": 3 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("Approved."),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);

        let mut tools = ToolRegistry::new();
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            approval_handler: Some(Arc::new(telos_agent::FixedDecisionHandler {
                decision: telos_agent::ApprovalDecision::Allow,
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
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 2, "b": 3 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("Denied."),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);

        let mut tools = ToolRegistry::new();
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            approval_handler: Some(Arc::new(telos_agent::FixedDecisionHandler {
                decision: telos_agent::ApprovalDecision::Deny { reason: "not today".into() },
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
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "add".into(),
                        arguments: json!({ "a": 2, "b": 3 }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("Modified."),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);

        let mut tools = ToolRegistry::new();
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig {
            permission_engine: Some(engine),
            approval_handler: Some(Arc::new(telos_agent::FixedDecisionHandler {
                decision: telos_agent::ApprovalDecision::Modify {
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
