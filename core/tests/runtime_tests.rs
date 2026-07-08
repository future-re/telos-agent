mod common;

use futures_util::StreamExt;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use common::*;
use telos_agent::*;

#[test]
fn multi_step_tool_loop_completes() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![
                        ContentBlock::Text(telos_agent::TextBlock {
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
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("The answer is 5."),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);

        let mut tools = ToolRegistry::new();
        tools.register(AddTool);

        let mut session = AgentSession::new(AgentConfig {
            base_system_prompt: Some("You are a coding agent.".into()),
            max_iterations: Some(4),
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

#[tokio::test]
async fn cancelling_during_provider_stream_returns_promptly() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let polled = Arc::new(tokio::sync::Notify::new());
    let cancellation = CancellationState::from_flag(Arc::clone(&cancelled));
    let config = AgentConfig { cancellation: cancellation.clone(), ..AgentConfig::default() };

    let handle = {
        let polled = Arc::clone(&polled);
        tokio::spawn(async move {
            let mut session = AgentSession::new(config).unwrap();
            let provider = HangingStreamProvider { polled };
            let tools = ToolRegistry::new();
            let mut stream = Box::pin(session.run_turn_stream(&provider, &tools, "hang"));
            while let Some(event) = stream.next().await {
                if let Err(err) = event {
                    return err;
                }
            }
            panic!("turn stream ended without surfacing cancellation");
        })
    };

    tokio::time::timeout(std::time::Duration::from_millis(100), polled.notified())
        .await
        .expect("provider stream was not polled");

    cancellation.cancel();

    let err = tokio::time::timeout(std::time::Duration::from_millis(100), handle)
        .await
        .expect("cancelled provider stream should return promptly");
    assert!(matches!(err.unwrap(), AgentError::Cancelled));
}

#[tokio::test]
async fn next_stream_turn_repairs_cancelled_tool_call_history() {
    let cancellation = CancellationState::new();
    let config = AgentConfig { cancellation: cancellation.clone(), ..AgentConfig::default() };
    let provider = MockProvider::new(vec![CompletionResponse {
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
    }]);
    let mut tools = ToolRegistry::new();
    tools.register(AddTool);
    let mut session = AgentSession::new(config).unwrap();

    let mut stream = Box::pin(session.run_turn_stream(&provider, &tools, "start"));
    let mut saw_cancelled = false;
    while let Some(event) = stream.next().await {
        match event {
            Ok(TurnEvent::Assistant(_)) => cancellation.cancel(),
            Ok(_) => {}
            Err(err) => {
                saw_cancelled = matches!(err, AgentError::Cancelled);
                break;
            }
        }
    }
    drop(stream);

    assert!(saw_cancelled, "turn should surface cancellation");
    assert!(matches!(
        session.messages().last(),
        Some(message) if message.role == telos_agent::Role::Assistant
            && message.tool_calls().any(|call| call.id == "call-1")
    ));

    cancellation.reset();
    let provider = MockProvider::new(vec![CompletionResponse {
        message: Message::assistant("continued"),
        stop_reason: StopReason::EndTurn,
        usage: None,
        model: None,
    }]);

    let events = session
        .run_turn_stream(&provider, &tools, "continue")
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let requests = provider.requests.lock().await;
    let repaired_request = &requests[0];
    assert!(repaired_request.messages.iter().any(|message| {
        message.role == telos_agent::Role::Tool
            && message.tool_results_iter().any(|result| {
                result.tool_call_id == "call-1"
                    && result.is_error
                    && result.content.to_string().contains("cancelled")
            })
    }));
    assert!(events.iter().any(|event| {
        matches!(event, TurnEvent::User(message) if message.text_content() == "continue")
    }));
}

#[test]
fn tool_calls_continue_even_when_stop_reason_is_end_turn() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
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
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("The answer is 10."),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
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
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "missing".into(),
                        arguments: json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("I could not run that tool."),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
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

#[tokio::test]
async fn runtime_input_after_tool_forces_thinking_reconsideration() {
    let provider = Arc::new(MockProvider::new(vec![
        CompletionResponse {
            message: Message {
                role: telos_agent::Role::Assistant,
                blocks: vec![ContentBlock::ToolCall(ToolCall {
                    id: "call-1".into(),
                    name: "wait".into(),
                    arguments: json!({}),
                })],
            },
            stop_reason: StopReason::ToolUse,
            usage: None,
            model: None,
        },
        CompletionResponse {
            message: Message::assistant("I reconsidered with the new input."),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        },
    ]));

    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let mut tools = ToolRegistry::new();
    tools.register(WaitTool { started: Arc::clone(&started), release: Arc::clone(&release) });

    let mut session = AgentSession::new(AgentConfig::default()).unwrap();
    let (input_tx, input_rx) = turn_input_channel();
    let provider_for_task = Arc::clone(&provider);

    let handle = tokio::spawn(async move {
        let erased = ErasedProvider(provider_for_task.as_ref());
        let events = session
            .run_turn_stream_with_input(&erased, &tools, "start", input_rx)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (session, events)
    });

    tokio::time::timeout(std::time::Duration::from_millis(100), started.notified())
        .await
        .expect("wait tool should start");
    input_tx.send("new constraint from user".to_string()).unwrap();
    release.notify_waiters();

    let (session, events) = handle.await.unwrap();
    let requests = provider.requests.lock().await;

    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].model_hint, Some(ModelHint::Thinking));
    let second_request_text =
        requests[1].messages.iter().map(Message::text_content).collect::<Vec<_>>().join("\n");
    assert!(second_request_text.contains("new constraint from user"));
    assert!(events.iter().any(|event| {
        matches!(event, TurnEvent::User(message) if message.text_content() == "new constraint from user")
    }));

    let tool_result_index = session
        .messages()
        .iter()
        .position(|message| message.role == telos_agent::Role::Tool)
        .expect("tool result should be stored");
    let injected_user_index = session
        .messages()
        .iter()
        .position(|message| {
            message.role == telos_agent::Role::User
                && message.text_content() == "new constraint from user"
        })
        .expect("runtime input should be stored");
    assert!(tool_result_index < injected_user_index);
}

#[test]
fn permission_denial_returns_structured_tool_error() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "deny".into(),
                        arguments: json!({}),
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
        tools.register(DenyTool);

        let mut session = AgentSession::new(AgentConfig::default()).unwrap();
        let result = session.run_turn(&provider, &tools, "try deny").await.unwrap();
        let tool_result_event =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();

        assert!(tool_result_event.text().contains("\"kind\":\"permission_denied\""));
    });
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
            model: None,
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
        let last_assistant =
            session.messages().iter().rfind(|m| m.role == telos_agent::Role::Assistant);
        assert_eq!(last_assistant.unwrap().text_content(), "hook-ran");
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
            model: None,
        }]);
        let tools = ToolRegistry::new();
        let mut session =
            AgentSession::new(AgentConfig { hooks: Arc::new(hooks), ..AgentConfig::default() })
                .unwrap();

        let result = session.run_turn(&provider, &tools, "hi").await.unwrap();
        // The hook output is appended as an assistant message, followed by a
        // system-reminder user message. The turn result should still expose
        // the model's own final answer.
        let last_assistant =
            session.messages().iter().rfind(|m| m.role == telos_agent::Role::Assistant);
        assert_eq!(last_assistant.unwrap().text_content(), "hook-ran");
        assert_eq!(result.final_message.text_content(), "model answer");
    });
}

#[test]
fn tool_result_budget_compacts_large_output() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "big".into(),
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
fn summary_history_compaction_triggers_when_over_budget() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        // Provider responses: tool_use, summary, final_end_turn
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "big".into(),
                        arguments: json!({}),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("summary result"),
                stop_reason: StopReason::EndTurn,
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
        tools.register(BigTool);

        let mut session = AgentSession::new(AgentConfig {
            compaction: Some(Arc::new(SummaryHistoryCompaction { max_tokens: 10, keep_recent: 2 })),
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
fn token_budget_triggers_auto_compaction() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message::assistant("summary"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("done"),
                stop_reason: StopReason::EndTurn,
                usage: Some(telos_agent::TokenUsage::new(10, 2)),
                model: None,
            },
        ]);
        let tools = ToolRegistry::new();
        let mut session = AgentSession::new(AgentConfig {
            base_system_prompt: Some("sys".into()),
            compaction: Some(Arc::new(SummaryHistoryCompaction { max_tokens: 50, keep_recent: 0 })),
            token_budget: Some(TokenBudget { max_tokens: 1_000, compact_at_tokens: 10 }),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "x".repeat(200)).await.unwrap();
        assert!(result.events.iter().any(|event| {
            matches!(event, TurnEvent::CompactionStarted { reason } if reason == "token_budget")
        }));
        assert!(result.events.iter().any(|event| {
            matches!(event, TurnEvent::ProviderUsage { input_tokens: 10, output_tokens: 2, .. })
        }));
    });
}

#[test]
fn thinking_blocks_are_separate_from_final_text() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message {
                role: telos_agent::Role::Assistant,
                blocks: vec![
                    ContentBlock::Thinking(telos_agent::ThinkingBlock {
                        text: "I need to reason about this.".into(),
                        signature: None,
                        is_redacted: false,
                    }),
                    ContentBlock::Text(telos_agent::TextBlock { text: "The answer is 7.".into() }),
                ],
            },
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
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
fn system_reminder_renders_with_tags() {
    use telos_agent::message::SystemReminder;
    let reminder = SystemReminder::Compaction { reason: "token_budget".into() };
    let text = reminder.render();
    assert!(text.contains("<system-reminder>"));
    assert!(text.contains("token_budget"));
    assert!(text.contains("</system-reminder>"));
}

#[test]
fn compaction_emits_system_reminder() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        // Provide extra responses so the compaction summary calls (which use
        // `provider.complete`) do not starve the actual turn completion.
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message::assistant("summary"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("summary"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("hi"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);
        let tools = ToolRegistry::new();
        let mut session = AgentSession::new(AgentConfig {
            token_budget: Some(TokenBudget { max_tokens: 1_000_000, compact_at_tokens: 1 }),
            compaction: Some(Arc::new(SummaryHistoryCompaction { max_tokens: 1, keep_recent: 0 })),
            ..AgentConfig::default()
        })
        .unwrap();

        let _ = session.run_turn(&provider, &tools, "hello").await.unwrap();
        let has_reminder = session.messages().iter().any(|m| {
            m.role == telos_agent::Role::System && m.text_content().contains("<system-reminder>")
        });
        assert!(has_reminder);
    });
}

#[test]
fn skill_discovery_emits_system_reminder() {
    use telos_agent::provider::{CompletionResponse, StopReason};
    use telos_agent::skills::{Skill, SkillRegistry, SkillSource};

    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("done"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        }]);
        let tools = ToolRegistry::new();
        let mut registry = SkillRegistry::new();
        registry.register(Skill {
            name: "rust-fix".into(),
            description: "Fix Rust compiler errors".into(),
            when_to_use: Some("When cargo check fails".into()),
            prompt: "Prompt".into(),
            arguments: vec![],
            body: "rust compile".into(),
            source: SkillSource::Bundled,
        });
        let registry = Arc::new(registry);
        let mut session = AgentSession::new(AgentConfig {
            skill_registry: Some(Arc::clone(&registry)),
            skill_injector: Some(Arc::new(telos_agent::SkillInjector::new(registry))),
            ..AgentConfig::default()
        })
        .unwrap();

        let _ = session.run_turn(&provider, &tools, "fix rust compile error").await.unwrap();
        let has_reminder = session.messages().iter().any(|m| {
            m.role == telos_agent::Role::System
                && m.text_content().contains("Recommended Skills")
                && m.text_content().contains("rust-fix")
        });
        assert!(has_reminder);
    });
}
