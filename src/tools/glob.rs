//! `glob` tool — list files matching a glob pattern under the workspace.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{display_relative, required_string};

/// Built-in glob tool. Read-only; safe to run concurrently.
pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Glob".into(),
            description: "List files matching a glob pattern under the current working directory."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "max_results": { "type": "integer" }
                },
                "required": ["pattern"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["glob"]
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string(arguments, "pattern").map(|_| ())
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        true
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let pattern = required_string(&arguments, "pattern")?;
        // Cap results so a pathological `**/*` doesn't dump millions of paths into the context.
        let max_results = arguments
            .get("max_results")
            .and_then(|value| value.as_u64())
            .unwrap_or(200) as usize;
        // Anchor the pattern at cwd so the model can write relative globs.
        let full_pattern = context.cwd.join(pattern).to_string_lossy().to_string();
        let mut matches = Vec::new();
        for entry in
            glob::glob(&full_pattern).map_err(|err| AgentError::Validation(err.to_string()))?
        {
            if matches.len() >= max_results {
                break;
            }
            if let Ok(path) = entry {
                // Display paths relative to cwd; absolute paths are noisy and leak the host layout.
                matches.push(display_relative(&context.cwd, &path));
            }
        }
        Ok(ToolOutput::json(json!({ "matches": matches })))
    }
}
