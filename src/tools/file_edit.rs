//! `file_edit` tool — exact-match find-and-replace on a single UTF-8 text file.
//!
//! Requires the `old` string to occur **exactly once** in the file. This is a
//! deliberate guard against ambiguous edits — if the model wants to replace a
//! common token it must include enough surrounding context to make the match
//! unique.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{required_string, resolve_workspace_path};

/// Built-in file-edit tool. Performs a single, unambiguous string replacement.
pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_edit".into(),
            description: "Replace an exact string in a UTF-8 text file.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old": { "type": "string" },
                    "new": { "type": "string" }
                },
                "required": ["path", "old", "new"]
            }),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string(arguments, "path")?;
        required_string(arguments, "old")?;
        required_string(arguments, "new")?;
        Ok(())
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Ask {
            reason: "file edit requires approval".into(),
        })
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let path = resolve_workspace_path(&context.cwd, required_string(&arguments, "path")?)?;
        let content =
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|err| AgentError::ToolExecution {
                    tool: "file_edit".into(),
                    message: err.to_string(),
                })?;
        let old = required_string(&arguments, "old")?;
        let new = required_string(&arguments, "new")?;
        // Reject ambiguous (0 or >1) matches so the model is forced to widen
        // the snippet rather than silently editing the wrong location.
        let count = content.matches(old).count();
        if count != 1 {
            return Err(AgentError::ToolExecution {
                tool: "file_edit".into(),
                message: format!("expected exactly one match, found {count}"),
            });
        }
        let updated = content.replacen(old, new, 1);
        tokio::fs::write(&path, updated)
            .await
            .map_err(|err| AgentError::ToolExecution {
                tool: "file_edit".into(),
                message: err.to_string(),
            })?;
        Ok(ToolOutput::json(json!({ "path": path, "replaced": true })))
    }
}
