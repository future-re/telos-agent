//! `grep` tool — substring search over files matched by a glob.
//!
//! Matches are *literal* substring searches (no regex). For each hit we emit
//! the path, 1-indexed line number, and the matched line — enough context for
//! the model to follow up with [`FileReadTool`].

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{display_relative, required_string};

/// Built-in grep tool. Read-only; safe to run concurrently.
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Grep".into(),
            description: "Search UTF-8 files for a literal text pattern.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "glob": { "type": "string" },
                    "max_results": { "type": "integer" }
                },
                "required": ["pattern"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["grep"]
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
        let pattern = required_string(&arguments, "pattern")?.to_string();
        // Default to a recursive glob so plain "grep for X" works out of the box.
        let file_glob = arguments
            .get("glob")
            .and_then(|value| value.as_str())
            .unwrap_or("**/*");
        let max_results = arguments
            .get("max_results")
            .and_then(|value| value.as_u64())
            .unwrap_or(200) as usize;
        let full_pattern = context.cwd.join(file_glob).to_string_lossy().to_string();
        let mut results = Vec::new();
        for entry in
            glob::glob(&full_pattern).map_err(|err| AgentError::Validation(err.to_string()))?
        {
            if results.len() >= max_results {
                break;
            }
            let Ok(path) = entry else {
                continue;
            };
            if !path.is_file() {
                continue;
            }
            // Silently skip files we can't read as UTF-8 (binary, permissions, etc.).
            let Ok(content) = tokio::fs::read_to_string(&path).await else {
                continue;
            };
            for (idx, line) in content.lines().enumerate() {
                if line.contains(&pattern) {
                    results.push(json!({
                        "path": display_relative(&context.cwd, &path),
                        "line": idx + 1,
                        "text": line,
                    }));
                    if results.len() >= max_results {
                        break;
                    }
                }
            }
        }
        Ok(ToolOutput::json(json!({ "matches": results })))
    }
}
