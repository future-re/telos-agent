//! Shared helpers used by the built-in tool implementations.
//!
//! Every function here is `pub(crate)` — internal to the crate, not part of
//! the public API surface.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::AgentError;
use crate::tool::ToolContext;

/// Extract a required string field from JSON arguments or return a validation error.
pub(crate) fn required_string<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, AgentError> {
    arguments
        .get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| AgentError::Validation(format!("missing string `{key}`")))
}

/// Extract the first available string field from JSON arguments.
pub(crate) fn required_string_any<'a>(
    arguments: &'a Value,
    keys: &[&str],
) -> Result<&'a str, AgentError> {
    for key in keys {
        if let Some(value) = arguments.get(*key).and_then(|value| value.as_str()) {
            return Ok(value);
        }
    }
    Err(AgentError::Validation(format!(
        "missing string `{}`",
        keys.join("` or `")
    )))
}

/// Extract an optional bool argument with a default.
pub(crate) fn optional_bool(arguments: &Value, key: &str, default: bool) -> bool {
    arguments
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

/// Extract an optional positive integer from any one of several keys.
pub(crate) fn optional_usize_any(arguments: &Value, keys: &[&str]) -> Option<usize> {
    keys.iter().find_map(|key| {
        arguments
            .get(*key)
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
    })
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
pub(crate) fn normalize_path(path: &Path) -> PathBuf {
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

/// Resolve a path against `cwd` and follow symlinks, verifying the final
/// canonical location still lies inside `cwd`.
///
/// This is the second line of defence against path traversal via symlinks:
/// `resolve_workspace_path` only normalises `.`/`..` lexically, so a symlink
/// inside `cwd` that points outside will slip through unless we canonicalise.
pub(crate) async fn canonicalize_within_cwd(
    cwd: &Path,
    path: &Path,
) -> Result<PathBuf, AgentError> {
    let canonical_cwd = tokio::fs::canonicalize(cwd)
        .await
        .map_err(|err| AgentError::ToolExecution {
            tool: "filesystem".into(),
            message: format!("failed to canonicalize cwd: {err}"),
        })?;

    // Try to canonicalise the target itself; if it does not exist yet (e.g. a
    // write to a new file), canonicalise its parent directory instead.
    let canonical_path = match tokio::fs::canonicalize(path).await {
        Ok(p) => p,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(cwd);
            let canonical_parent = tokio::fs::canonicalize(parent)
                .await
                .map_err(|err| AgentError::ToolExecution {
                    tool: "filesystem".into(),
                    message: format!("failed to canonicalize parent directory: {err}"),
                })?;
            canonical_parent.join(path.file_name().unwrap_or_default())
        }
        Err(err) => {
            return Err(AgentError::ToolExecution {
                tool: "filesystem".into(),
                message: format!("failed to canonicalize path: {err}"),
            })
        }
    };

    if !canonical_path.starts_with(&canonical_cwd) {
        return Err(AgentError::PermissionDenied(format!(
            "path escapes cwd after following symlinks: {}",
            path.display()
        )));
    }

    Ok(canonical_path)
}

/// Return a comparable millisecond timestamp for a file's last modification time.
pub(crate) async fn modified_timestamp_ms(path: &Path) -> Result<u128, AgentError> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|err| AgentError::ToolExecution {
            tool: "filesystem".into(),
            message: err.to_string(),
        })?;
    metadata
        .modified()
        .map_err(|err| AgentError::ToolExecution {
            tool: "filesystem".into(),
            message: err.to_string(),
        })?
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|err| AgentError::ToolExecution {
            tool: "filesystem".into(),
            message: err.to_string(),
        })
}

/// Reject writes to files that haven't been read or have changed since being read.
///
/// Shared by [`FileWriteTool`] and [`FileEditTool`] — both enforce the same
/// read-before-mutate invariant. `tool_name` is used in error messages so the
/// model knows which tool's guard was triggered.
pub(crate) async fn ensure_file_was_read_and_unchanged(
    tool_name: &str,
    context: &ToolContext,
    path: &Path,
    current_content: &str,
) -> Result<(), AgentError> {
    let last_read = context.read_file_state.lock().await.get(path).cloned();
    let Some(last_read) = last_read else {
        return Err(AgentError::ToolExecution {
            tool: tool_name.into(),
            message: "File has not been read yet. Read it first before writing to it.".into(),
        });
    };
    if last_read.is_partial_view {
        return Err(AgentError::ToolExecution {
            tool: tool_name.into(),
            message: "File has only been partially read. Read the full file before writing to it."
                .into(),
        });
    }
    if current_content != last_read.content {
        return Err(AgentError::ToolExecution {
            tool: tool_name.into(),
            message:
                "File has been modified since read, either by the user or by a linter. Read it again before attempting to write it."
                    .into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- normalize_path ---

    #[test]
    fn normalize_path_resolves_dots() {
        let p = normalize_path(Path::new("/a/b/../c/./d"));
        assert_eq!(p, PathBuf::from("/a/c/d"));
    }

    #[test]
    fn normalize_path_handles_parent_beyond_root() {
        let p = normalize_path(Path::new("/a/../../../b"));
        assert_eq!(p, PathBuf::from("/b"));
    }

    // --- resolve_workspace_path ---

    #[test]
    fn resolve_workspace_accepts_path_under_cwd() {
        let cwd = Path::new("/workspace");
        let resolved = resolve_workspace_path(cwd, "src/main.rs").unwrap();
        assert_eq!(resolved, PathBuf::from("/workspace/src/main.rs"));
    }

    #[test]
    fn resolve_workspace_rejects_escape() {
        let cwd = Path::new("/workspace");
        let err = resolve_workspace_path(cwd, "../etc/passwd").unwrap_err();
        assert!(matches!(err, AgentError::PermissionDenied(_)));
    }

    #[test]
    fn resolve_workspace_accepts_absolute_path_under_cwd() {
        let cwd = Path::new("/workspace");
        let resolved = resolve_workspace_path(cwd, "/workspace/sub/file.txt").unwrap();
        assert_eq!(resolved, PathBuf::from("/workspace/sub/file.txt"));
    }

    #[test]
    fn resolve_workspace_rejects_absolute_path_outside_cwd() {
        let cwd = Path::new("/workspace");
        let err = resolve_workspace_path(cwd, "/etc/passwd").unwrap_err();
        assert!(matches!(err, AgentError::PermissionDenied(_)));
    }

    // --- required_string ---

    #[test]
    fn required_string_extracts_successfully() {
        let args = json!({"file_path": "/tmp/test.txt"});
        assert_eq!(required_string(&args, "file_path").unwrap(), "/tmp/test.txt");
    }

    #[test]
    fn required_string_errors_on_missing_key() {
        let args = json!({"other": "value"});
        assert!(
            matches!(required_string(&args, "file_path"), Err(AgentError::Validation(_)))
        );
    }

    // --- required_string_any ---

    #[test]
    fn required_string_any_finds_first_match() {
        let args = json!({"a": "first", "b": "second"});
        assert_eq!(required_string_any(&args, &["b", "a"]).unwrap(), "second");
    }

    #[test]
    fn required_string_any_errors_when_none_match() {
        let args = json!({"x": 1});
        assert!(
            matches!(
                required_string_any(&args, &["a", "b"]),
                Err(AgentError::Validation(_))
            )
        );
    }

    // --- optional helpers ---

    #[test]
    fn optional_bool_returns_default_when_missing() {
        assert!(!optional_bool(&json!({}), "flag", false));
        assert!(optional_bool(&json!({}), "flag", true));
    }

    #[test]
    fn optional_bool_reads_value_when_present() {
        assert!(optional_bool(&json!({"flag": true}), "flag", false));
    }

    #[test]
    fn optional_usize_any_finds_value_across_keys() {
        let args = json!({"lines": 42});
        assert_eq!(optional_usize_any(&args, &["lines", "count"]), Some(42));
    }

    #[test]
    fn optional_usize_any_returns_none_when_absent() {
        assert_eq!(optional_usize_any(&json!({}), &["lines"]), None);
    }

    // --- display_relative ---

    #[test]
    fn display_relative_strips_cwd_prefix() {
        assert_eq!(
            display_relative(Path::new("/home"), Path::new("/home/user/file.txt")),
            "user/file.txt"
        );
    }

    #[test]
    fn display_relative_falls_back_to_absolute() {
        assert_eq!(
            display_relative(Path::new("/home"), Path::new("/other/file.txt")),
            "/other/file.txt"
        );
    }
}
