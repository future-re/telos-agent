mod common;

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

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

struct CwdProbeTool {
    seen: Arc<Mutex<Option<PathBuf>>>,
}

#[async_trait]
impl Tool for CwdProbeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cwd_probe".into(),
            description: "Record the tool cwd for tests.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        }
    }

    async fn invoke(
        &self,
        _arguments: serde_json::Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        *self.seen.lock().await = Some(context.cwd.clone());
        Ok(ToolOutput::text(context.cwd.display().to_string()))
    }
}

fn init_git_repo(path: &Path) {
    let run = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap_or_else(|err| panic!("failed to run git {args:?}: {err}"));
        assert!(
            output.status.success(),
            "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test User"]);
    std::fs::write(path.join("README.md"), "test repo\n").unwrap();
    run(&["add", "README.md"]);
    run(&["commit", "-m", "init"]);
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
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("outer done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);
        let inner_provider = Arc::new(MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("inner answer"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
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
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("outer done"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
        ]);
        let inner_provider = Arc::new(MockProvider::new(vec![
            CompletionResponse {
                message: Message::assistant("Security: found XSS"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            },
            CompletionResponse {
                message: Message::assistant("Performance: slow query"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
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
        model: None,
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
async fn subagent_background_worktree_records_task_path() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path());
    let task_dir = tempfile::tempdir().unwrap();
    let task_manager = Arc::new(TaskManager::new(task_dir.path().to_path_buf()));
    let provider = Arc::new(MockProvider::new(vec![CompletionResponse {
        message: Message::assistant("background worktree answer"),
        stop_reason: StopReason::EndTurn,
        usage: None,
        model: None,
    }]));
    let tool = SubagentTool::new(
        provider,
        ToolRegistry::new(),
        AgentConfig { task_manager: Some(task_manager.clone()), ..AgentConfig::default() },
    );

    let output = tool
        .invoke(
            json!({
                "description": "Background worktree",
                "prompt": "solve in isolation",
                "run_in_background": true,
                "isolation": "worktree"
            }),
            tool_context(repo.path().to_path_buf()),
        )
        .await
        .unwrap()
        .content;

    let task_id = output["task_id"].as_str().unwrap();
    let worktree_path =
        output["worktree_path"].as_str().expect("launch result should include worktree path");
    assert!(Path::new(worktree_path).exists());

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

    let task = completed.expect("background worktree task should complete");
    assert_eq!(task.output.as_deref(), Some("background worktree answer"));
    assert_eq!(task.worktree_path.as_deref(), Some(worktree_path));
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
async fn subagent_worktree_isolation_runs_child_tools_in_worktree() {
    let repo = tempfile::tempdir().unwrap();
    init_git_repo(repo.path());
    let seen_cwd = Arc::new(Mutex::new(None));
    let mut child_tools = ToolRegistry::new();
    child_tools.register(CwdProbeTool { seen: seen_cwd.clone() });
    let provider = Arc::new(MockProvider::new(vec![
        CompletionResponse {
            message: Message {
                role: Role::Assistant,
                blocks: vec![ContentBlock::ToolCall(ToolCall {
                    id: "probe-1".into(),
                    name: "cwd_probe".into(),
                    arguments: json!({}),
                })],
            },
            stop_reason: StopReason::ToolUse,
            usage: None,
            model: None,
        },
        CompletionResponse {
            message: Message::assistant("worktree done"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        },
    ]));
    let tool = SubagentTool::new(provider, child_tools, AgentConfig::default());

    let output = tool
        .invoke(
            json!({
                "description": "Worktree probe",
                "prompt": "record cwd",
                "isolation": "worktree"
            }),
            tool_context(repo.path().to_path_buf()),
        )
        .await
        .unwrap()
        .content;

    let worktree_path = output["worktree_path"].as_str().expect("worktree path should be returned");
    let seen = seen_cwd.lock().await.clone().expect("cwd probe should run");
    assert_eq!(seen, PathBuf::from(worktree_path));
    assert_ne!(seen, repo.path());
    assert!(seen.ends_with(
        Path::new(".worktrees").join("subagents").join(output["agent_id"].as_str().unwrap())
    ));
    assert_eq!(output["final_text"], "worktree done");
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
    let (command, args, description) = plugin_uppercase_command_spec();

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
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"]
        },
        "command": command,
        "args": args,
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

    let (tools, _hooks, _skills, _mcp, _prompt, result) =
        config.apply_plugins(tools, hooks, skills, mcp, prompt);
    result.unwrap();

    // Verify the tool is registered with namespace
    let tool = tools.get("plugin__mytool__uppercase");
    assert!(tool.is_ok(), "plugin tool should be registered: {:?}", tool.err());
}

#[cfg(windows)]
fn plugin_uppercase_command_spec() -> (&'static str, serde_json::Value, &'static str) {
    (
        "powershell",
        json!([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$input = [Console]::In.ReadToEnd() | ConvertFrom-Json; [Console]::Out.Write($input.text.ToUpperInvariant())"
        ]),
        "Converts text to uppercase using PowerShell",
    )
}

#[cfg(not(windows))]
fn plugin_uppercase_command_spec() -> (&'static str, serde_json::Value, &'static str) {
    (
        "/bin/sh",
        json!([
            "-c",
            "input=$(cat); printf '%s' \"$input\" | sed -n 's/.*\"text\":\"\\([^\"]*\\)\".*/\\1/p' | tr '[:lower:]' '[:upper:]'"
        ]),
        "Converts text to uppercase using POSIX shell tools",
    )
}
