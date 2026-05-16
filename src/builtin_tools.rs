use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::error::AgentError;
use crate::tool::{
    PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry,
};

pub fn register_core_tools(registry: &mut ToolRegistry) {
    registry.register(ShellTool);
    registry.register(FileReadTool);
    registry.register(FileWriteTool);
    registry.register(FileEditTool);
    registry.register(GlobTool);
    registry.register(GrepTool);
}

pub struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "shell".into(),
            description: "Run a shell command in the current working directory.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string(arguments, "command").map(|_| ())
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        let command = required_string(arguments, "command")?;
        if is_obviously_read_only_command(command) {
            Ok(PermissionDecision::Allow)
        } else {
            Ok(PermissionDecision::Ask {
                reason: "shell command may mutate state".into(),
            })
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let command = required_string(&arguments, "command")?;
        let mut child = Command::new("sh");
        child
            .arg("-c")
            .arg(command)
            .current_dir(&context.cwd)
            .envs(context.env.iter())
            .env_remove("ENV")
            .env_remove("BASH_ENV");
        let output = child
            .output()
            .await
            .map_err(|err| AgentError::ToolExecution {
                tool: "shell".into(),
                message: err.to_string(),
            })?;

        Ok(ToolOutput::json(json!({
            "status": output.status.code(),
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        })))
    }
}

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

pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_write".into(),
            description: "Write a UTF-8 text file relative to the current working directory."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string(arguments, "path")?;
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
        let path = resolve_workspace_path(&context.cwd, required_string(&arguments, "path")?)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| AgentError::ToolExecution {
                    tool: "file_write".into(),
                    message: err.to_string(),
                })?;
        }
        let content = required_string(&arguments, "content")?;
        tokio::fs::write(&path, content)
            .await
            .map_err(|err| AgentError::ToolExecution {
                tool: "file_write".into(),
                message: err.to_string(),
            })?;
        Ok(ToolOutput::json(json!({ "path": path, "written": true })))
    }
}

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

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "glob".into(),
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
        let max_results = arguments
            .get("max_results")
            .and_then(|value| value.as_u64())
            .unwrap_or(200) as usize;
        let full_pattern = context.cwd.join(pattern).to_string_lossy().to_string();
        let mut matches = Vec::new();
        for entry in
            glob::glob(&full_pattern).map_err(|err| AgentError::Validation(err.to_string()))?
        {
            if matches.len() >= max_results {
                break;
            }
            if let Ok(path) = entry {
                matches.push(display_relative(&context.cwd, &path));
            }
        }
        Ok(ToolOutput::json(json!({ "matches": matches })))
    }
}

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "grep".into(),
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

fn required_string<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, AgentError> {
    arguments
        .get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| AgentError::Validation(format!("missing string `{key}`")))
}

fn resolve_workspace_path(cwd: &Path, path: &str) -> Result<PathBuf, AgentError> {
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        cwd.join(path)
    };
    let normalized = normalize_path(&candidate);
    let normalized_cwd = normalize_path(cwd);
    if !normalized.starts_with(&normalized_cwd) {
        return Err(AgentError::PermissionDenied(format!(
            "path escapes cwd: {}",
            candidate.display()
        )));
    }
    Ok(normalized)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn display_relative(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn is_obviously_read_only_command(command: &str) -> bool {
    let first = command
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches(';');
    matches!(
        first,
        "cat" | "head" | "tail" | "ls" | "pwd" | "rg" | "grep" | "find" | "wc" | "sed" | "git"
    )
}
