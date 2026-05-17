//! `Write` tool — overwrite a UTF-8 text file inside the workspace.
//!
//! Always returns [`PermissionDecision::Ask`] — writes are mutating, so the
//! host gets the final say.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{modified_timestamp_ms, required_string, required_string_any, resolve_workspace_path};

/// Built-in file-write tool. Writes (and creates) text files inside the workspace.
pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Write".into(),
            description: "Create or overwrite a UTF-8 text file. Existing files must be read first with Read.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["file_path", "content"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["file_write"]
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string_any(arguments, &["file_path", "path"])?;
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
        let input_path = required_string_any(&arguments, &["file_path", "path"])?;
        let path = resolve_workspace_path(&context.cwd, input_path)?;
        let content = required_string(&arguments, "content")?;
        if let Ok(existing_content) = tokio::fs::read_to_string(&path).await {
            ensure_file_was_read_and_unchanged(&context, &path, &existing_content).await?;
        }
        // Create any missing parent directories so the model can write nested paths in one call.
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| AgentError::ToolExecution {
                    tool: "Write".into(),
                    message: err.to_string(),
                })?;
        }
        tokio::fs::write(&path, content)
            .await
            .map_err(|err| AgentError::ToolExecution {
                tool: "Write".into(),
                message: err.to_string(),
            })?;
        let timestamp_ms = modified_timestamp_ms(&path).await?;
        context.read_file_state.lock().await.insert(
            path.clone(),
            crate::tool::FileReadRecord {
                content: content.to_string(),
                timestamp_ms,
                is_partial_view: false,
                offset: None,
                limit: None,
            },
        );
        Ok(ToolOutput::json(json!({
            "file_path": input_path,
            "path": path,
            "written": true,
        })))
    }
}

async fn ensure_file_was_read_and_unchanged(
    context: &ToolContext,
    path: &std::path::Path,
    current_content: &str,
) -> Result<(), AgentError> {
    let last_read = context.read_file_state.lock().await.get(path).cloned();
    let Some(last_read) = last_read else {
        return Err(AgentError::ToolExecution {
            tool: "Write".into(),
            message: "File has not been read yet. Read it first before writing to it.".into(),
        });
    };
    if last_read.is_partial_view {
        return Err(AgentError::ToolExecution {
            tool: "Write".into(),
            message: "File has only been partially read. Read the full file before writing to it."
                .into(),
        });
    }
    if current_content != last_read.content {
        return Err(AgentError::ToolExecution {
            tool: "Write".into(),
            message: "File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.".into(),
        });
    }
    Ok(())
}
