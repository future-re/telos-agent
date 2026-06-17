//! `Read` tool — read a UTF-8 text file with optional line slicing.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::FileReadRecord;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{
    canonicalize_within_cwd, modified_timestamp_ms, optional_usize_any, required_string_any,
    resolve_workspace_path,
};

/// Built-in file-read tool. Read-only; safe to run concurrently.
pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Read".into(),
            description: "Read a UTF-8 text file. Use this before editing an existing file. \
The returned `content` prefixes each line with its 1-indexed line number for display; \
provide the original file text (without line-number prefixes) to `Edit` when editing.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Path to the file to read, absolute or relative to cwd." },
                    "offset": { "type": "integer", "description": "1-indexed line number to start reading from." },
                    "limit": { "type": "integer", "description": "Maximum number of lines to read." }
                },
                "required": ["file_path"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["file_read"]
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string_any(arguments, &["file_path", "path"]).map(|_| ())
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        true
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let input_path = required_string_any(&arguments, &["file_path", "path"])?;
        let path = resolve_workspace_path(&context.cwd, input_path)?;
        let path = canonicalize_within_cwd(&context.cwd, &path).await?;
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|err| AgentError::ToolExecution {
                tool: "Read".into(),
                message: friendly_file_error(&context.cwd, &path, err),
            })?;
        if metadata.len() > context.max_file_read_bytes as u64 {
            return Err(AgentError::ToolExecution {
                tool: "Read".into(),
                message: format!(
                    "file exceeds maximum read size ({} bytes): {}",
                    context.max_file_read_bytes,
                    path.display()
                ),
            });
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|err| AgentError::ToolExecution {
                tool: "Read".into(),
                message: friendly_file_error(&context.cwd, &path, err),
            })?;
        if bytes.contains(&0) {
            return Err(AgentError::ToolExecution {
                tool: "Read".into(),
                message:
                    "file appears to be binary; Read only supports UTF-8 text in tiny_agent_core"
                        .into(),
            });
        }
        let content = String::from_utf8(bytes).map_err(|_| AgentError::ToolExecution {
            tool: "Read".into(),
            message: "file is not valid UTF-8 text".into(),
        })?;
        let timestamp_ms = modified_timestamp_ms(&path).await?;
        // `offset` is 1-indexed for human readability; clamp to >= 1.
        let start_line = optional_usize_any(&arguments, &["offset", "start_line"])
            .unwrap_or(1)
            .max(1);
        let max_lines = optional_usize_any(&arguments, &["limit", "max_lines"]);
        // Prefix every emitted line with its 1-indexed line number — gives the
        // model an unambiguous anchor for follow-up edits.
        let lines = content
            .lines()
            .enumerate()
            .skip(start_line.saturating_sub(1))
            .take(max_lines.unwrap_or(usize::MAX))
            .map(|(idx, line)| format!("{}: {}", idx + 1, line))
            .collect::<Vec<_>>();
        let line_count = content.lines().count();
        let is_partial_view = start_line > 1 || max_lines.is_some_and(|limit| limit < line_count);
        context.read_file_state.lock().await.insert(
            path.clone(),
            FileReadRecord {
                content: content.clone(),
                timestamp_ms,
                is_partial_view,
                offset: Some(start_line),
                limit: max_lines,
            },
        );

        let rendered = if content.is_empty() {
            "<system-reminder>Warning: the file exists but the contents are empty.</system-reminder>".to_string()
        } else if lines.is_empty() {
            format!(
                "<system-reminder>Warning: the file exists but is shorter than the provided offset ({start_line}). The file has {line_count} lines.</system-reminder>"
            )
        } else {
            lines.join("\n")
        };

        Ok(ToolOutput::json(json!({
            "file_path": input_path,
            "path": path,
            "content": rendered,
            "start_line": start_line,
            "total_lines": line_count,
        })))
    }
}

fn friendly_file_error(
    cwd: &std::path::Path,
    path: &std::path::Path,
    err: std::io::Error,
) -> String {
    if err.kind() == std::io::ErrorKind::NotFound {
        format!(
            "File does not exist. Current working directory: {}. Requested path: {}",
            cwd.display(),
            path.display()
        )
    } else {
        err.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn ctx(cwd: std::path::PathBuf, max_bytes: usize) -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            turn_id: 1,
            cwd,
            env: HashMap::new(),
            messages: std::sync::Arc::new(vec![]),
            progress: None,
            read_file_state: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            timeout: None,
            max_file_read_bytes: max_bytes,
        }
    }

    #[tokio::test]
    async fn rejects_oversized_file() {
        let dir = std::env::temp_dir().join("tiny_agent_read_size_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("big.txt"), "x".repeat(100)).unwrap();

        let tool = FileReadTool;
        let result = tool
            .invoke(json!({ "file_path": "big.txt" }), ctx(dir.clone(), 50))
            .await;
        assert!(matches!(result, Err(AgentError::ToolExecution { .. })));
        assert!(
            result.unwrap_err().to_string().contains("maximum read size"),
            "error should mention maximum read size"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
