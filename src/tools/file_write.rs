//! `file_write` tool — overwrite a UTF-8 text file inside the workspace.
//!
//! Always returns [`PermissionDecision::Ask`] — writes are mutating, so the
//! host gets the final say.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{required_string, resolve_workspace_path};

/// Built-in file-write tool. Writes (and creates) text files inside the workspace.
pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_write".into(),
            description: "Write a UTF-8 text file relative to the current working directory."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string(arguments, "path")?;
        required_string(arguments, "content")?;
        Ok(())
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Ask {
            reason: "file write requires approval".into(),
        })
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let path = resolve_workspace_path(&context.cwd, required_string(&arguments, "path")?)?;
        // Create any missing parent directories so the model can write nested paths in one call.
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| AgentError::ToolExecution {
                    tool: "file_write".into(),
                    message: err.to_string(),
                })?;
        }
        let content = required_string(&arguments, "content")?;
        tokio::fs::write(&path, content)
            .await
            .map_err(|err| AgentError::ToolExecution {
                tool: "file_write".into(),
                message: err.to_string(),
            })?;
        Ok(ToolOutput::json(json!({ "path": path, "written": true })))
    }
}
