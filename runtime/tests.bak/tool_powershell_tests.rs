mod common;

use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use telos_agent::*;

fn ctx(cwd: std::path::PathBuf, env: HashMap<String, String>) -> ToolContext {
    ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        tool_call_id: None,
        cwd,
        env,
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        timeout: None,
        max_file_read_bytes: usize::MAX,
    }
}

fn powershell_available() -> bool {
    std::process::Command::new("pwsh")
        .arg("-NoProfile")
        .arg("-Command")
        .arg("$PSVersionTable.PSVersion")
        .output()
        .is_ok()
        || std::process::Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg("$PSVersionTable.PSVersion")
            .output()
            .is_ok()
}

#[test]
fn register_core_tools_includes_powershell() {
    let mut tools = ToolRegistry::new();
    register_core_tools_with_shell(&mut tools, DefaultShell::PowerShell);
    assert!(tools.get("PowerShell").is_ok());
    assert_eq!(tools.get("shell").unwrap().definition().name, "PowerShell");
}

#[tokio::test]
async fn safe_command_runs_with_clean_environment_when_powershell_exists() {
    if !powershell_available() {
        eprintln!("skipping: PowerShell not installed");
        return;
    }
    unsafe { std::env::set_var("TELOS_AGENT_SECRET", "leaked") };
    let tool = PowerShellTool;
    let mut env = HashMap::new();
    env.insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
    let output = tool
        .invoke(
            json!({ "command": "Write-Output $env:TELOS_AGENT_SECRET" }),
            ctx(std::env::temp_dir(), env),
        )
        .await
        .unwrap();
    assert_eq!(output.content["stdout"].as_str().unwrap().trim(), "");
    unsafe { std::env::remove_var("TELOS_AGENT_SECRET") };
}

#[tokio::test]
async fn configured_env_is_passed_when_powershell_exists() {
    if !powershell_available() {
        eprintln!("skipping: PowerShell not installed");
        return;
    }
    let tool = PowerShellTool;
    let mut env = HashMap::new();
    env.insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
    env.insert("MY_PS_VAR".into(), "present".into());
    let output = tool
        .invoke(json!({ "command": "Write-Output $env:MY_PS_VAR" }), ctx(std::env::temp_dir(), env))
        .await
        .unwrap();
    assert_eq!(output.content["stdout"].as_str().unwrap().trim(), "present");
}

#[tokio::test]
async fn timeout_returns_error_when_powershell_exists() {
    if !powershell_available() {
        eprintln!("skipping: PowerShell not installed");
        return;
    }
    let tool = PowerShellTool;
    let mut env = HashMap::new();
    env.insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
    let err = tool
        .invoke(
            json!({ "command": "Start-Sleep -Seconds 5", "timeout_ms": 10 }),
            ctx(std::env::temp_dir(), env),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("timed out"));
}
