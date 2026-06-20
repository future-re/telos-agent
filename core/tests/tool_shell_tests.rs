use futures_util::StreamExt;
use serde_json::json;

use telos_agent::*;

#[tokio::test]
async fn cancelling_during_bash_tool_kills_running_command() {
    let temp = tempfile::tempdir().unwrap();
    let pid_path = temp.path().join("command.pid");
    let command = format!("echo $$ > {}; sleep 10", pid_path.display());
    let provider = MockProvider::new(vec![CompletionResponse {
        message: Message {
            role: telos_agent::Role::Assistant,
            blocks: vec![ContentBlock::ToolCall(ToolCall {
                id: "bash-call".into(),
                name: "Bash".into(),
                arguments: json!({ "command": command, "timeout_ms": 10_000 }),
            })],
        },
        stop_reason: StopReason::ToolUse,
        usage: None,
    }]);
    let cancellation = CancellationState::new();
    let mut engine = PermissionEngine::new();
    engine.add_rule(PermissionRule::allow_tool("Bash"));
    let mut session = AgentSession::new(AgentConfig {
        cwd: temp.path().to_path_buf(),
        cancellation: cancellation.clone(),
        permission_engine: Some(engine),
        ..AgentConfig::default()
    })
    .unwrap();
    let mut tools = ToolRegistry::new();
    register_core_tools(&mut tools);

    let handle = tokio::spawn(async move {
        let mut stream = Box::pin(session.run_turn_stream(&provider, &tools, "run bash"));
        while let Some(event) = stream.next().await {
            if let Err(err) = event {
                return err;
            }
        }
        panic!("turn stream ended without surfacing cancellation");
    });

    let pid = tokio::time::timeout(std::time::Duration::from_millis(500), async {
        loop {
            if let Ok(pid) = tokio::fs::read_to_string(&pid_path).await {
                break pid.trim().parse::<i32>().unwrap();
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("bash command did not write its pid");

    cancellation.cancel();

    let err = tokio::time::timeout(std::time::Duration::from_millis(250), handle)
        .await
        .expect("cancelled bash command should return promptly");
    assert!(matches!(err.unwrap(), AgentError::Cancelled));

    #[cfg(unix)]
    {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let still_running = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        assert!(!still_running, "bash command pid {pid} was still running after cancellation");
    }
}

#[tokio::test]
#[cfg(unix)]
async fn cancelling_during_bash_tool_kills_child_processes() {
    let temp = tempfile::tempdir().unwrap();
    let child_pid_path = temp.path().join("child.pid");
    let command =
        format!("sleep 10 & child=$!; echo $child > {}; wait $child", child_pid_path.display());
    let provider = MockProvider::new(vec![CompletionResponse {
        message: Message {
            role: telos_agent::Role::Assistant,
            blocks: vec![ContentBlock::ToolCall(ToolCall {
                id: "bash-call".into(),
                name: "Bash".into(),
                arguments: json!({ "command": command, "timeout_ms": 10_000 }),
            })],
        },
        stop_reason: StopReason::ToolUse,
        usage: None,
    }]);
    let cancellation = CancellationState::new();
    let mut engine = PermissionEngine::new();
    engine.add_rule(PermissionRule::allow_tool("Bash"));
    let mut session = AgentSession::new(AgentConfig {
        cwd: temp.path().to_path_buf(),
        cancellation: cancellation.clone(),
        permission_engine: Some(engine),
        ..AgentConfig::default()
    })
    .unwrap();
    let mut tools = ToolRegistry::new();
    register_core_tools(&mut tools);

    let handle = tokio::spawn(async move {
        let mut stream = Box::pin(session.run_turn_stream(&provider, &tools, "run bash"));
        while let Some(event) = stream.next().await {
            if let Err(err) = event {
                return err;
            }
        }
        panic!("turn stream ended without surfacing cancellation");
    });

    let child_pid = tokio::time::timeout(std::time::Duration::from_millis(500), async {
        loop {
            if let Ok(pid) = tokio::fs::read_to_string(&child_pid_path).await {
                break pid.trim().parse::<i32>().unwrap();
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("bash command did not write its child pid");

    cancellation.cancel();

    let err = tokio::time::timeout(std::time::Duration::from_millis(250), handle)
        .await
        .expect("cancelled bash command should return promptly");
    assert!(matches!(err.unwrap(), AgentError::Cancelled));

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let still_running = std::process::Command::new("kill")
        .args(["-0", &child_pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    assert!(
        !still_running,
        "bash child process pid {child_pid} was still running after cancellation"
    );
}
