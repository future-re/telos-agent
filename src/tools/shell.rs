//! `shell` tool — run an arbitrary shell command in the workspace cwd.
//!
//! Permission policy: commands that look obviously read-only (see
//! [`is_obviously_read_only_command`]) are auto-approved; everything else
//! requires host approval via [`PermissionDecision::Ask`].

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{is_obviously_read_only_command, required_string};

/// Built-in shell tool. Spawns `sh -c <command>` inside the workspace.
pub struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "shell".into(),
            description: "Run a shell command in the current working directory.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
        }
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
        if is_obviously_read_only_command(command) {
            Ok(PermissionDecision::Allow)
        } else {
            Ok(PermissionDecision::Ask {
                reason: "shell command may mutate state".into(),
            })
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let command = required_string(&arguments, "command")?;
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
        let output = child
            .output()
            .await
            .map_err(|err| AgentError::ToolExecution {
                tool: "shell".into(),
                message: err.to_string(),
            })?;

        // We don't translate non-zero exit codes into errors — many shell
        // utilities use them for control flow, and the model can read the
        // status code from the JSON payload.
        Ok(ToolOutput::json(json!({
            "status": output.status.code(),
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        })))
    }
}
