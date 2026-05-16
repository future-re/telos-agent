use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{required_string, resolve_workspace_path};

pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_read".into(),
            description: "Read a UTF-8 text file relative to the current working directory.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "start_line": { "type": "integer" },
                    "max_lines": { "type": "integer" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string(arguments, "path").map(|_| ())
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        true
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
                    tool: "file_read".into(),
                    message: err.to_string(),
                })?;
        let start_line = arguments
            .get("start_line")
            .and_then(|value| value.as_u64())
            .unwrap_or(1)
            .max(1) as usize;
        let max_lines = arguments
            .get("max_lines")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize);
        let lines = content
            .lines()
            .enumerate()
            .skip(start_line.saturating_sub(1))
            .take(max_lines.unwrap_or(usize::MAX))
            .map(|(idx, line)| format!("{}: {}", idx + 1, line))
            .collect::<Vec<_>>();
        Ok(ToolOutput::json(json!({
            "path": path,
            "content": lines.join("\n"),
        })))
    }
}
