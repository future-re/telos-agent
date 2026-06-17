//! `grep` tool — substring search over files matched by a glob.
//!
//! Matches are *literal* substring searches (no regex). For each hit we emit
//! the path, 1-indexed line number, and the matched line — enough context for
//! the model to follow up with [`FileReadTool`].

use async_trait::async_trait;
use serde_json::{Value, json};

use std::path::Path;

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{display_relative, is_within_cwd, required_string};

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
        // Reject absolute globs — they would bypass the cwd anchor.
        if Path::new(file_glob).is_absolute() {
            return Err(AgentError::PermissionDenied(format!(
                "absolute glob patterns are not allowed: {file_glob}"
            )));
        }
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
            // Defensive: `../foo` style globs can still resolve outside cwd.
            if !is_within_cwd(&context.cwd, &path) {
                continue;
            }
            // Skip files that exceed the configured read budget.
            if let Ok(metadata) = tokio::fs::metadata(&path).await
                && metadata.len() > context.max_file_read_bytes as u64
            {
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
            messages: std::sync::Arc::new(vec![]),
            progress: None,
            read_file_state: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            timeout: None,
            max_file_read_bytes: usize::MAX,
        }
    }

    #[tokio::test]
    async fn rejects_absolute_glob() {
        let tool = GrepTool;
        let result = tool
            .invoke(
                json!({ "pattern": "root", "glob": "/etc/*" }),
                ctx(std::path::PathBuf::from("/workspace")),
            )
            .await;
        assert!(matches!(result, Err(AgentError::PermissionDenied(_))));
    }

    #[tokio::test]
    async fn skips_files_outside_cwd() {
        let dir = std::env::temp_dir().join("tiny_agent_grep_escape_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("inside.txt"), "match").unwrap();
        std::fs::write(dir.join("outside.txt"), "match").unwrap();

        let tool = GrepTool;
        let output = tool
            .invoke(
                json!({ "pattern": "match", "glob": "../**/*.txt" }),
                ctx(dir.join("sub")),
            )
            .await
            .unwrap();
        let matches = output.content["matches"].as_array().unwrap();
        assert!(matches.iter().all(|m| !m["path"].as_str().unwrap().contains("outside")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
