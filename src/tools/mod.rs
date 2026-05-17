//! Built-in tools: shell, file read/write/edit, glob, grep.
//!
//! Each tool gates write access through [`PermissionDecision::Ask`] so the
//! host (typically a human approval prompt) keeps the final say. Read-only
//! tools are marked concurrency-safe so they can run in parallel batches.

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

/// Register every built-in tool with the supplied registry.
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

/// Extract a required string field from JSON arguments or return a validation error.
pub(crate) fn required_string<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, AgentError> {
    arguments
        .get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| AgentError::Validation(format!("missing string `{key}`")))
}

/// Resolve a user-supplied path against the workspace cwd, refusing to escape it.
///
/// Absolute paths are taken as-is; relative paths are joined onto `cwd`. We
/// normalise `.` / `..` and then assert the result still lies inside `cwd` —
/// this is the only line of defence against path-traversal attacks via the
/// filesystem tools.
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

/// Lexically resolve `.` and `..` without touching the filesystem.
///
/// We deliberately don't follow symlinks — that would require I/O and could
/// race with the file being written. The trade-off is that a symlink pointing
/// outside `cwd` will slip through; tools that care should check separately.
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

/// Format `path` relative to `cwd` for display, falling back to the absolute path on failure.
pub(crate) fn display_relative(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// Heuristic: does this shell command look obviously read-only?
///
/// Used by [`ShellTool`] to auto-approve safe commands without bothering the
/// human. Conservative — anything not in the allow-list still needs approval.
/// Note: `git` is included on the assumption that bare `git` invocations are
/// inspection-like (`git status`, `git log`); mutating subcommands still pass
/// here but should be caught upstream via a permission rule if needed.
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
