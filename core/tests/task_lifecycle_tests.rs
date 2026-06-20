use std::sync::Arc;

use serde_json::json;
use telos_agent::tool::FileReadState;
use telos_agent::{
    Message, Task, TaskManager, TaskOutputTool, TaskStatus, TaskStopTool, Tool, ToolContext,
};

fn test_context() -> ToolContext {
    ToolContext {
        session_id: "test-session".into(),
        turn_id: 0,
        tool_call_id: None,
        cwd: std::env::current_dir().unwrap(),
        env: std::collections::HashMap::new(),
        messages: Arc::new(Vec::<Message>::new()),
        progress: None,
        read_file_state: FileReadState::default(),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    }
}

#[test]
fn task_manager_persists_subagent_metadata_and_terminal_statuses() {
    let dir = tempfile::tempdir().unwrap();
    let manager = TaskManager::new(dir.path().to_path_buf());
    let mut task = Task {
        id: "agent_test".into(),
        subject: "Investigate parser".into(),
        description: "Find the parser issue".into(),
        status: TaskStatus::InProgress,
        blocked_by: vec![],
        blocks: vec![],
        output: None,
        kind: Some("subagent".into()),
        agent_id: Some("agent_test".into()),
        agent_type: Some("Explore".into()),
        worktree_path: Some("/tmp/repo/.worktrees/subagents/agent_test".into()),
        error: None,
    };
    manager.create(task.clone());

    manager.set_output("agent_test", "parser issue found".into());
    manager.fail("agent_test", "provider failed".into());

    let failed = manager.get("agent_test").unwrap();
    assert_eq!(failed.status, TaskStatus::Failed);
    assert_eq!(failed.output.as_deref(), Some("parser issue found"));
    assert_eq!(failed.error.as_deref(), Some("provider failed"));
    assert_eq!(failed.kind.as_deref(), Some("subagent"));
    assert_eq!(failed.agent_type.as_deref(), Some("Explore"));
    assert_eq!(failed.worktree_path.as_deref(), Some("/tmp/repo/.worktrees/subagents/agent_test"));

    task.id = "agent_cancelled".into();
    task.status = TaskStatus::InProgress;
    task.error = None;
    manager.create(task);
    manager.cancel("agent_cancelled", "stopped by user".into());

    let reopened = TaskManager::new(dir.path().to_path_buf());
    let cancelled = reopened.get("agent_cancelled").unwrap();
    assert_eq!(cancelled.status, TaskStatus::Cancelled);
    assert_eq!(cancelled.error.as_deref(), Some("stopped by user"));
}

#[tokio::test]
async fn task_output_tool_returns_output_and_subagent_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(TaskManager::new(dir.path().to_path_buf()));
    manager.create(Task {
        id: "agent_output".into(),
        subject: "Run worker".into(),
        description: "Background worker".into(),
        status: TaskStatus::Completed,
        blocked_by: vec![],
        blocks: vec![],
        output: Some("worker finished".into()),
        kind: Some("subagent".into()),
        agent_id: Some("agent_output".into()),
        agent_type: Some("general-purpose".into()),
        worktree_path: None,
        error: None,
    });

    let output = TaskOutputTool::new(manager)
        .invoke(json!({"task_id": "agent_output"}), test_context())
        .await
        .unwrap()
        .content;

    assert_eq!(output["task_id"], "agent_output");
    assert_eq!(output["status"], "completed");
    assert_eq!(output["output"], "worker finished");
    assert_eq!(output["kind"], "subagent");
    assert_eq!(output["agent_type"], "general-purpose");
}

#[tokio::test]
async fn task_stop_tool_cancels_running_task() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(TaskManager::new(dir.path().to_path_buf()));
    manager.create(Task {
        id: "agent_running".into(),
        subject: "Run worker".into(),
        description: "Background worker".into(),
        status: TaskStatus::InProgress,
        blocked_by: vec![],
        blocks: vec![],
        output: None,
        kind: Some("subagent".into()),
        agent_id: Some("agent_running".into()),
        agent_type: Some("Explore".into()),
        worktree_path: None,
        error: None,
    });

    let output = TaskStopTool::new(manager.clone())
        .invoke(json!({"task_id": "agent_running"}), test_context())
        .await
        .unwrap()
        .content;

    assert_eq!(output["task_id"], "agent_running");
    assert_eq!(output["status"], "cancelled");
    assert_eq!(manager.get("agent_running").unwrap().status, TaskStatus::Cancelled);
}
