//! `Edit` tool — exact-match find-and-replace on a single UTF-8 text file.
//!
//! Requires the `old` string to occur **exactly once** in the file. This is a
//! deliberate guard against ambiguous edits — if the model wants to replace a
//! common token it must include enough surrounding context to make the match
//! unique.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{ensure_file_was_read_and_unchanged, modified_timestamp_ms, optional_bool, required_string_any, resolve_workspace_path};

/// Built-in file-edit tool. Performs a single, unambiguous string replacement.
pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Edit".into(),
            description: "Replace an exact string in a UTF-8 text file. Existing files must be read first with Read.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Path to the file to edit, absolute or relative to cwd." },
                    "old_string": { "type": "string", "description": "Exact string to replace. Use an empty string only to create a new file." },
                    "new_string": { "type": "string", "description": "Replacement string." },
                    "replace_all": { "type": "boolean", "description": "Replace every occurrence. Defaults to false." }
                },
                "required": ["file_path", "old_string", "new_string"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["file_edit"]
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string_any(arguments, &["file_path", "path"])?;
        required_string_any(arguments, &["old_string", "old"])?;
        required_string_any(arguments, &["new_string", "new"])?;
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
        let input_path = required_string_any(&arguments, &["file_path", "path"])?;
        let path = resolve_workspace_path(&context.cwd, input_path)?;
        if path.extension().and_then(|ext| ext.to_str()) == Some("ipynb") {
            return Err(AgentError::ToolExecution {
                tool: "Edit".into(),
                message: "File is a Jupyter Notebook. Use a notebook-aware edit tool instead."
                    .into(),
            });
        }
        let old = required_string_any(&arguments, &["old_string", "old"])?;
        let new = required_string_any(&arguments, &["new_string", "new"])?;
        if old == new {
            return Err(AgentError::ToolExecution {
                tool: "Edit".into(),
                message: "No changes to make: old_string and new_string are exactly the same."
                    .into(),
            });
        }
        let replace_all = optional_bool(&arguments, "replace_all", false);

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && old.is_empty() => {
                String::new()
            }
            Err(err) => {
                return Err(AgentError::ToolExecution {
                    tool: "Edit".into(),
                    message: err.to_string(),
                });
            }
        };

        if !old.is_empty() {
            ensure_file_was_read_and_unchanged("Edit", &context, &path, &content).await?;
        } else if !content.trim().is_empty() {
            return Err(AgentError::ToolExecution {
                tool: "Edit".into(),
                message: "Cannot create new file - file already exists.".into(),
            });
        }

        // Reject ambiguous (0 or >1) matches so the model is forced to widen
        // the snippet rather than silently editing the wrong location.
        let count = if old.is_empty() {
            1
        } else {
            content.matches(old).count()
        };
        if count == 0 {
            return Err(AgentError::ToolExecution {
                tool: "Edit".into(),
                message: format!("String to replace not found in file.\nString: {old}"),
            });
        }
        if count > 1 && !replace_all {
            return Err(AgentError::ToolExecution {
                tool: "Edit".into(),
                message: format!(
                    "Found {count} matches of the string to replace, but replace_all is false. Provide more context to uniquely identify the instance or set replace_all to true.\nString: {old}"
                ),
            });
        }
        let updated = if old.is_empty() {
            new.to_string()
        } else if replace_all {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| AgentError::ToolExecution {
                    tool: "Edit".into(),
                    message: err.to_string(),
                })?;
        }
        tokio::fs::write(&path, updated)
            .await
            .map_err(|err| AgentError::ToolExecution {
                tool: "Edit".into(),
                message: err.to_string(),
            })?;
        let updated_content =
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|err| AgentError::ToolExecution {
                    tool: "Edit".into(),
                    message: err.to_string(),
                })?;
        let timestamp_ms = modified_timestamp_ms(&path).await?;
        context.read_file_state.lock().await.insert(
            path.clone(),
            crate::tool::FileReadRecord {
                content: updated_content,
                timestamp_ms,
                is_partial_view: false,
                offset: None,
                limit: None,
            },
        );
        Ok(ToolOutput::json(json!({
            "file_path": input_path,
            "path": path,
            "replaced": true,
            "replace_all": replace_all,
        })))
    }
}

