mod common;

use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use common::HangingStreamProvider;
use telos_agent::tool::FileReadState;
use telos_agent::*;

fn tool_context(cwd: std::path::PathBuf) -> ToolContext {
    ToolContext {
        session_id: "test-session".into(),
        turn_id: 1,
        tool_call_id: Some("call-1".into()),
        cwd,
        env: HashMap::new(),
        messages: Arc::new(Vec::new()),
        progress: None,
        read_file_state: FileReadState::default(),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    }
}

#[test]
fn subagent_tool_runs_in_process_agent() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let outer_provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
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
fn subagent_fork_mode_runs_multiple_lenses() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let outer_provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message {
                    role: telos_agent::Role::Assistant,
                    blocks: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call-1".into(),
                        name: "subagent".into(),
                        arguments: json!({
                            "prompt": "analyze",
                            "mode": "fork",
                            "forks": [
                                {
                                    "lens": "security",
                                    "system_prompt": "You are a security expert",
                                    "task": "Find security issues"
                                },
                                {
                                    "lens": "performance",
                                    "system_prompt": "You are a performance expert",
                                    "task": "Find perf issues"
                                }
                            ]
                        }),
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
        let inner_provider = Arc::new(MockProvider::new(vec![
            CompletionResponse {
                message: Message::assistant("Security: found XSS"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
            CompletionResponse {
                message: Message::assistant("Performance: slow query"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]));
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

        let result = session.run_turn(&outer_provider, &tools, "analyze code").await.unwrap();
        let tool_result =
            result.events.iter().find(|event| matches!(event, TurnEvent::ToolResult(_))).unwrap();
        let text = tool_result.text();
        assert!(text.contains("Security: found XSS"), "expected security result, got: {text}");
        assert!(
            text.contains("Performance: slow query"),
            "expected performance result, got: {text}"
        );
    });
}

#[tokio::test]
async fn subagent_background_mode_records_task_output() {
    let task_dir = tempfile::tempdir().unwrap();
    let task_manager = Arc::new(TaskManager::new(task_dir.path().to_path_buf()));
    let inner_provider = Arc::new(MockProvider::new(vec![CompletionResponse {
        message: Message::assistant("background inner answer"),
        stop_reason: StopReason::EndTurn,
        usage: None,
    }]));
    let tool = SubagentTool::new(
        inner_provider,
        ToolRegistry::new(),
        AgentConfig { task_manager: Some(task_manager.clone()), ..AgentConfig::default() },
    );

    let output = tool
        .invoke(
            json!({
                "description": "Background solve",
                "prompt": "solve inside",
                "run_in_background": true
            }),
            tool_context(std::env::current_dir().unwrap()),
        )
        .await
        .unwrap()
        .content;

    assert_eq!(output["status"], "async_launched");
    assert_eq!(output["agent_type"], "general-purpose");
    let task_id = output["task_id"].as_str().unwrap();

    let mut completed = None;
    for _ in 0..50 {
        if let Some(task) = task_manager.get(task_id)
            && task.status == TaskStatus::Completed
        {
            completed = Some(task);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let task = completed.expect("background task should complete");
    assert_eq!(task.output.as_deref(), Some("background inner answer"));
    assert_eq!(task.kind.as_deref(), Some("subagent"));
    assert_eq!(task.agent_id.as_deref(), Some(task_id));
    assert_eq!(task.agent_type.as_deref(), Some("general-purpose"));
}

#[tokio::test]
async fn subagent_background_task_can_be_stopped() {
    let task_dir = tempfile::tempdir().unwrap();
    let task_manager = Arc::new(TaskManager::new(task_dir.path().to_path_buf()));
    let polled = Arc::new(tokio::sync::Notify::new());
    let provider = Arc::new(HangingStreamProvider { polled: polled.clone() });
    let tool = SubagentTool::new(
        provider,
        ToolRegistry::new(),
        AgentConfig { task_manager: Some(task_manager.clone()), ..AgentConfig::default() },
    );

    let output = tool
        .invoke(
            json!({
                "description": "Long worker",
                "prompt": "wait inside",
                "run_in_background": true
            }),
            tool_context(std::env::current_dir().unwrap()),
        )
        .await
        .unwrap()
        .content;
    let task_id = output["task_id"].as_str().unwrap().to_string();

    tokio::time::timeout(std::time::Duration::from_secs(1), polled.notified())
        .await
        .expect("background provider should start");

    let stop = TaskStopTool::new(task_manager.clone())
        .invoke(json!({"task_id": task_id}), tool_context(std::env::current_dir().unwrap()))
        .await
        .unwrap()
        .content;
    assert_eq!(stop["status"], "cancelled");

    let mut cancelled = None;
    for _ in 0..50 {
        if let Some(task) = task_manager.get(output["task_id"].as_str().unwrap())
            && task.status == TaskStatus::Cancelled
        {
            cancelled = Some(task);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let task = cancelled.expect("background task should be cancelled");
    assert_eq!(task.kind.as_deref(), Some("subagent"));
    assert_eq!(task.agent_type.as_deref(), Some("general-purpose"));
}

#[tokio::test]
async fn plugin_tool_integration() {
    use telos_agent::{
        AgentConfig,
        hooks::HookRegistry,
        mcp::McpManager,
        plugin::{PluginId, PluginRegistry},
        prompt::PromptAssembly,
        skills::SkillRegistry,
        tool::ToolRegistry,
    };
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let plugin_dir = tmp.path().join("installed").join("mytool@test");
    std::fs::create_dir_all(plugin_dir.join("tools")).unwrap();

    // Write plugin.json
    let manifest = serde_json::json!({
        "name": "mytool",
        "version": "1.0.0",
        "tools": ["./tools/uppercase.json"]
    });
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Write a tool spec
    let tool_spec = serde_json::json!({
        "name": "uppercase",
        "description": "Converts text to uppercase using tr",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"]
        },
        "command": "tr",
        "args": ["[:lower:]", "[:upper:]"],
        "permission": "allow",
        "isConcurrencySafe": true
    });
    std::fs::write(
        plugin_dir.join("tools").join("uppercase.json"),
        serde_json::to_string_pretty(&tool_spec).unwrap(),
    )
    .unwrap();

    // Register and enable the plugin
    let mut registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();
    let id = PluginId::parse("mytool@test").unwrap();
    registry.enable(&id).unwrap();

    // Apply plugins via AgentConfig
    let tools = ToolRegistry::new();
    let hooks = HookRegistry::new();
    let skills = SkillRegistry::new();
    let mcp = McpManager::new(std::collections::HashMap::new());
    let prompt = PromptAssembly::new();

    let config = AgentConfig {
        plugin_registry: Some(std::sync::Arc::new(registry)),
        ..AgentConfig::default()
    };

    let (tools, _hooks, _skills, _mcp, _prompt) =
        config.apply_plugins(tools, hooks, skills, mcp, prompt).unwrap();

    // Verify the tool is registered with namespace
    let tool = tools.get("plugin__mytool__uppercase");
    assert!(tool.is_ok(), "plugin tool should be registered: {:?}", tool.err());
}
