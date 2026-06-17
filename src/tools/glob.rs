//! `glob` tool — list files matching a glob pattern under the workspace.

use async_trait::async_trait;
use serde_json::{Value, json};

use std::path::Path;

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{display_relative, is_within_cwd, required_string};

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
        // Reject absolute patterns outright — they would bypass the cwd anchor.
        if Path::new(pattern).is_absolute() {
            return Err(AgentError::PermissionDenied(format!(
                "absolute glob patterns are not allowed: {pattern}"
            )));
        }
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
                // Defensive: `../foo` style patterns can still resolve outside cwd.
                if !is_within_cwd(&context.cwd, &path) {
                    continue;
                }
                // Display paths relative to cwd; absolute paths are noisy and leak the host layout.
                matches.push(display_relative(&context.cwd, &path));
            }
        }
        Ok(ToolOutput::json(json!({ "matches": matches })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn ctx(cwd: std::path::PathBuf) -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            turn_id: 1,
            cwd,
            env: HashMap::new(),
            messages: vec![],
            progress: None,
            read_file_state: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            timeout: None,
            max_file_read_bytes: usize::MAX,
        }
    }

    #[tokio::test]
    async fn rejects_absolute_pattern() {
        let tool = GlobTool;
        let result = tool
            .invoke(json!({ "pattern": "/etc/*" }), ctx(std::path::PathBuf::from("/workspace")))
            .await;
        assert!(matches!(result, Err(AgentError::PermissionDenied(_))));
    }

    #[tokio::test]
    async fn skips_matches_outside_cwd() {
        let dir = std::env::temp_dir().join("tiny_agent_glob_escape_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("file.txt"), "x").unwrap();
        std::fs::write(dir.join("outside.txt"), "x").unwrap();

        let tool = GlobTool;
        let output = tool
            .invoke(json!({ "pattern": "../**/*" }), ctx(dir.join("sub")))
            .await
            .unwrap();
        let matches = output.content["matches"].as_array().unwrap();
        // Should not include ../outside.txt even though the glob matches it.
        assert!(matches.iter().all(|m| !m.as_str().unwrap().contains("outside")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
