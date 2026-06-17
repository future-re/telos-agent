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
        let timeout_ms = optional_usize_any(&arguments, &["timeout_ms"])
            .unwrap_or(120_000)
            .max(1) as u64;
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
            .envs(context.env.iter())
            // Strip startup scripts so the inherited env doesn't silently
            // alter behaviour (PROMPT_COMMAND, etc.).
            .env_remove("ENV")
            .env_remove("BASH_ENV");
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
