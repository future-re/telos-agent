use crate::error::AgentError;
use crate::tool::ToolRegistry;

mod shell;
mod file_read;
mod file_write;
mod file_edit;
mod glob;
mod grep;

pub use shell::ShellTool;
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use file_edit::FileEditTool;
pub use glob::GlobTool;
pub use grep::GrepTool;

pub fn register_core_tools(registry: &mut ToolRegistry) {
    registry.register(ShellTool);
    registry.register(FileReadTool);
    registry.register(FileWriteTool);
    registry.register(FileEditTool);
    registry.register(GlobTool);
    registry.register(GrepTool);
}

use std::path::{Path, PathBuf};
use serde_json::Value;

pub(crate) fn required_string<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, AgentError> {
    arguments
        .get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| AgentError::Validation(format!("missing string `{key}`")))
}

pub(crate) fn resolve_workspace_path(cwd: &Path, path: &str) -> Result<PathBuf, AgentError> {
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

pub(crate) fn display_relative(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

pub(crate) fn is_obviously_read_only_command(command: &str) -> bool {
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
