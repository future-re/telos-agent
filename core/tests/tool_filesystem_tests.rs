use futures_util::StreamExt;
use serde_json::json;

use telos_agent::*;

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
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "file_read".into(),
                        arguments: json!({ "file_path": "sample.txt" }),
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
fn builtin_file_read_accepts_absolute_path_under_cwd() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir = std::env::temp_dir()
            .join(format!("tiny-agent-file-read-absolute-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sample.txt");
        std::fs::write(&file, "absolute\npath\n").unwrap();

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "Read".into(),
                        arguments: json!({ "file_path": file.to_string_lossy() }),
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
        let mut session =
            AgentSession::new(AgentConfig { cwd: dir.clone(), ..AgentConfig::default() }).unwrap();

        let result = session.run_turn(&provider, &tools, "read absolute").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("1: absolute"));
        assert!(tool_result.text().contains("2: path"));

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn builtin_file_read_rejects_absolute_path_outside_cwd() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir = std::env::temp_dir()
            .join(format!("tiny-agent-file-read-absolute-cwd-test-{}", std::process::id()));
        let outside = std::env::temp_dir()
            .join(format!("tiny-agent-file-read-absolute-outside-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, "super-secret").unwrap();

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "Read".into(),
                        arguments: json!({ "file_path": secret.to_string_lossy() }),
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
        let mut session =
            AgentSession::new(AgentConfig { cwd: dir.clone(), ..AgentConfig::default() }).unwrap();

        let result = session.run_turn(&provider, &tools, "read outside").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(
            tool_result.text().contains("permission_denied")
                || tool_result.text().contains("escapes cwd"),
            "{}",
            tool_result.text()
        );
        assert!(!tool_result.text().contains("super-secret"));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    });
}

#[cfg(unix)]
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
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "Read".into(),
                        arguments: json!({ "file_path": "link.txt" }),
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

#[cfg(windows)]
#[test]
fn file_read_rejects_windows_symlink_escape() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir = std::env::temp_dir()
            .join(format!("tiny-agent-windows-symlink-test-{}", std::process::id()));
        let outside = std::env::temp_dir()
            .join(format!("tiny-agent-windows-symlink-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "super-secret").unwrap();
        if let Err(err) =
            std::os::windows::fs::symlink_file(outside.join("secret.txt"), dir.join("link.txt"))
        {
            eprintln!("skipping Windows symlink test; symlink creation failed: {err}");
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_dir_all(&outside);
            return;
        }

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "Read".into(),
                        arguments: json!({ "file_path": "link.txt" }),
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
        assert!(!tool_result.text().contains("super-secret"));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    });
}

#[cfg(windows)]
#[test]
fn file_write_accepts_windows_backslash_relative_path() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir = std::env::temp_dir()
            .join(format!("tiny-agent-write-backslash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "Write".into(),
                        arguments: json!({
                            "file_path": "nested\\sample.txt",
                            "content": "windows path\n"
                        }),
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
            cwd: dir.clone(),
            permission_engine: Some({
                let mut engine = PermissionEngine::new();
                engine.add_rule(PermissionRule::allow_tool("Write"));
                engine
            }),
            ..AgentConfig::default()
        })
        .unwrap();

        let result = session.run_turn(&provider, &tools, "write").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        assert!(tool_result.text().contains("\"written\":true"), "{}", tool_result.text());
        assert_eq!(
            std::fs::read_to_string(dir.join("nested").join("sample.txt")).unwrap(),
            "windows path\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    });
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
                    role: telos_agent::Role::Assistant,
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
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "Read".into(),
                        arguments: json!({ "file_path": "sample.txt" }),
                    })],
                },
                stop_reason: StopReason::ToolUse,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
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
