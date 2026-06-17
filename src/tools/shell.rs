//! `Bash` tool — run an arbitrary shell command in the workspace cwd.
//!
//! Permission policy: commands that look obviously read-only (see
//! [`is_obviously_read_only_command`]) are auto-approved; everything else
//! requires host approval via [`PermissionDecision::Ask`].

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{optional_usize_any, required_string};
use crate::bash_security::{CommandSafety, analyze as analyze_command_safety};

/// Built-in shell tool. Spawns `sh -c <command>` inside the workspace.
pub struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Bash".into(),
            description: "Run a bash command in the current working directory. Prefer Read/Edit/Write/Glob/Grep for file operations.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "description": { "type": "string" },
                    "timeout_ms": { "type": "integer", "description": "Maximum runtime in milliseconds. Defaults to 120000." }
                },
                "required": ["command"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["shell"]
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string(arguments, "command").map(|_| ())
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        let command = required_string(arguments, "command")?;
        // Shell commands are extremely powerful and can read arbitrary files or
        // exfiltrate data even when they use a "read-only" looking basename.
        // Require explicit approval by default; callers can opt into auto-approval
        // via the global permission engine.
        match analyze_command_safety(command) {
            CommandSafety::Safe => Ok(PermissionDecision::Allow),
            CommandSafety::NeedsReview { reason } => Ok(PermissionDecision::Ask {
                reason: format!("shell command needs review: {reason}"),
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let command = required_string(&arguments, "command")?;
        let timeout_ms =
            optional_usize_any(&arguments, &["timeout_ms"]).unwrap_or(120_000).max(1) as u64;
        if let Some(tx) = &context.progress {
            let _ = tx.send(crate::tool::ToolProgress {
                tool_call_id: None,
                message: format!("running command with {timeout_ms}ms timeout"),
                data: Some(json!({ "command": command, "timeout_ms": timeout_ms })),
            });
        }
        let mut child = Command::new("sh");
        child
            .arg("-c")
            .arg(command)
            .current_dir(&context.cwd)
            // Start with a clean environment and only add what the caller
            // explicitly configured. Inheriting the parent process environment
            // would leak secrets (API keys, tokens) to arbitrary tool calls.
            .env_clear()
            .envs(context.env.iter());
        child.kill_on_drop(true);
        let output = timeout(Duration::from_millis(timeout_ms), child.output())
            .await
            .map_err(|_| AgentError::ToolExecution {
                tool: "Bash".into(),
                message: format!("Command timed out after {timeout_ms}ms"),
            })?
            .map_err(|err| AgentError::ToolExecution {
                tool: "Bash".into(),
                message: err.to_string(),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(AgentError::ToolExecution {
                tool: "Bash".into(),
                message: format!(
                    "Command failed with exit code {:?}\nstdout:\n{}\nstderr:\n{}",
                    output.status.code(),
                    trim_large_output(&stdout),
                    trim_large_output(&stderr)
                ),
            });
        }
        Ok(ToolOutput::json(json!({
            "status": output.status.code(),
            "success": output.status.success(),
            "stdout": trim_large_output(&stdout),
            "stderr": trim_large_output(&stderr),
        })))
    }
}

fn trim_large_output(output: &str) -> String {
    const MAX_CHARS: usize = 20_000;
    if output.chars().count() <= MAX_CHARS {
        return output.to_string();
    }
    let preview = output.chars().take(MAX_CHARS).collect::<String>();
    format!("{preview}\n<truncated output after {MAX_CHARS} chars>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn ctx(cwd: std::path::PathBuf, env: HashMap<String, String>) -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            turn_id: 1,
            cwd,
            env,
            messages: std::sync::Arc::new(vec![]),
            progress: None,
            read_file_state: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            timeout: None,
            max_file_read_bytes: usize::MAX,
        }
    }

    #[tokio::test]
    async fn safe_command_runs_with_clean_environment() {
        // Even if the parent process has SECRET set, the shell command should
        // not see it because ShellTool clears the environment before adding
        // only the configured variables.
        unsafe { std::env::set_var("TINY_AGENT_SECRET", "leaked") };
        let tool = ShellTool;
        let mut env = HashMap::new();
        env.insert("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into());
        let output = tool
            .invoke(json!({ "command": "echo $TINY_AGENT_SECRET" }), ctx(std::env::temp_dir(), env))
            .await
            .unwrap();
        let stdout = output.content["stdout"].as_str().unwrap();
        assert_eq!(stdout.trim(), "");
        unsafe { std::env::remove_var("TINY_AGENT_SECRET") };
    }

    #[tokio::test]
    async fn configured_env_is_passed_through() {
        let tool = ShellTool;
        let mut env = HashMap::new();
        env.insert("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into());
        env.insert("MY_VAR".into(), "present".into());
        let output = tool
            .invoke(json!({ "command": "echo $MY_VAR" }), ctx(std::env::temp_dir(), env))
            .await
            .unwrap();
        let stdout = output.content["stdout"].as_str().unwrap();
        assert_eq!(stdout.trim(), "present");
    }
}
