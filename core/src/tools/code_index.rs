use async_trait::async_trait;
use serde_json::{Value, json};

use crate::code_index::CodeIndex;
use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

pub struct CodeSearchTool;
pub struct CodeContextTool;
pub struct CodeIndexRefreshTool;

#[async_trait]
impl Tool for CodeSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "CodeSearch".into(),
            description:
                "Search the local code index and return exact file paths and line numbers.".into(),
            input_schema: json!({"type":"object","properties":{
                "query":{"type":"string"},
                "path_prefix":{"type":"string","description":"Optional path substring filter."},
                "max_results":{"type":"integer","default":50},
                "case_sensitive":{"type":"boolean","default":false}
            },"required":["query"]}),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["code_search"]
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use CodeSearch to search the indexed repository before broad filesystem reads. \
Results include path, 1-indexed line number, and matching line text.",
        )
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        true
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let query = required_string(&arguments, "query")?.to_string();
        let path_prefix = arguments.get("path_prefix").and_then(Value::as_str).map(str::to_string);
        let max_results =
            arguments.get("max_results").and_then(Value::as_u64).unwrap_or(50).min(500) as usize;
        let case_sensitive =
            arguments.get("case_sensitive").and_then(Value::as_bool).unwrap_or(false);
        let root = context.cwd.clone();
        tokio::task::spawn_blocking(move || {
            let index = CodeIndex::load_or_refresh(root).map_err(|err| {
                AgentError::ToolExecution { tool: "CodeSearch".into(), message: err.to_string() }
            })?;
            let matches = index.search(&query, path_prefix.as_deref(), max_results, case_sensitive);
            Ok(ToolOutput::json(json!({
                "index_path": CodeIndex::index_path(&index.root),
                "count": matches.len(),
                "matches": matches,
            })))
        })
        .await
        .map_err(|err| AgentError::ToolExecution {
            tool: "CodeSearch".into(),
            message: format!("code index task panicked: {err}"),
        })?
    }
}

#[async_trait]
impl Tool for CodeContextTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "CodeContext".into(),
            description: "Return nearby indexed code lines for a path and line number.".into(),
            input_schema: json!({"type":"object","properties":{
                "path":{"type":"string"},
                "line":{"type":"integer"},
                "before":{"type":"integer","default":5},
                "after":{"type":"integer","default":5}
            },"required":["path","line"]}),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["code_context"]
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        true
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let path = required_string(&arguments, "path")?.to_string();
        let line = arguments.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
        let before = arguments.get("before").and_then(Value::as_u64).unwrap_or(5).min(100) as usize;
        let after = arguments.get("after").and_then(Value::as_u64).unwrap_or(5).min(100) as usize;
        let root = context.cwd.clone();
        tokio::task::spawn_blocking(move || {
            let index = CodeIndex::load_or_refresh(root).map_err(|err| {
                AgentError::ToolExecution { tool: "CodeContext".into(), message: err.to_string() }
            })?;
            let lines = index.context(&path, line, before, after).ok_or_else(|| {
                AgentError::ToolExecution {
                    tool: "CodeContext".into(),
                    message: format!("path not found in code index: {path}"),
                }
            })?;
            Ok(ToolOutput::json(json!({"path": path, "line": line, "lines": lines})))
        })
        .await
        .map_err(|err| AgentError::ToolExecution {
            tool: "CodeContext".into(),
            message: format!("code index task panicked: {err}"),
        })?
    }
}

#[async_trait]
impl Tool for CodeIndexRefreshTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "CodeIndexRefresh".into(),
            description: "Refresh the local code index under .telos/index/code_index.json.".into(),
            input_schema: json!({"type":"object","properties":{}}),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["code_index_refresh"]
    }

    async fn invoke(
        &self,
        _arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let root = context.cwd.clone();
        tokio::task::spawn_blocking(move || {
            let index = CodeIndex::refresh(root).map_err(|err| AgentError::ToolExecution {
                tool: "CodeIndexRefresh".into(),
                message: err.to_string(),
            })?;
            Ok(ToolOutput::json(json!({
                "index_path": CodeIndex::index_path(&index.root),
                "files": index.files.len(),
            })))
        })
        .await
        .map_err(|err| AgentError::ToolExecution {
            tool: "CodeIndexRefresh".into(),
            message: format!("code index task panicked: {err}"),
        })?
    }
}

fn required_string<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, AgentError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AgentError::Validation(format!("missing `{key}`")))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{CodeContextTool, CodeIndexRefreshTool, CodeSearchTool};
    use crate::code_index::CodeIndex;
    use crate::tool::{Tool, ToolContext};

    fn test_context(cwd: std::path::PathBuf) -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            turn_id: 1,
            tool_call_id: None,
            cwd,
            env: Default::default(),
            messages: Arc::new(vec![]),
            progress: None,
            read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
            timeout: None,
            max_file_read_bytes: 50 * 1024 * 1024,
        }
    }

    #[tokio::test]
    async fn refresh_and_search_normalize_nested_paths_to_forward_slashes() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("src").join("windows");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("mod.rs"), "fn windows_path() {}\n").unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        let refresh = CodeIndexRefreshTool.invoke(json!({}), ctx.clone()).await.unwrap().content;
        assert_eq!(refresh["index_path"], json!(CodeIndex::index_path(dir.path())));

        let search = CodeSearchTool
            .invoke(json!({"query": "windows_path", "path_prefix": "src/windows"}), ctx.clone())
            .await
            .unwrap()
            .content;
        assert_eq!(search["count"], 1);
        assert_eq!(search["matches"][0]["path"], "src/windows/mod.rs");
        assert!(!search["matches"][0]["path"].as_str().unwrap().contains('\\'));

        let context = CodeContextTool
            .invoke(json!({"path": "src/windows/mod.rs", "line": 1, "before": 0, "after": 0}), ctx)
            .await
            .unwrap()
            .content;
        assert_eq!(context["lines"][0]["text"], "fn windows_path() {}");
    }
}
