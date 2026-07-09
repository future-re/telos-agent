//! Structured diagnostics for tool execution failures.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::error::AgentError;

/// Normalized classes for failures produced by the tool executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureKind {
    ValidationError,
    PermissionDenied,
    PermissionRequired,
    PermissionError,
    ToolNotFound,
    ExecutionError,
    ExecutionPanic,
}

impl ToolFailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolFailureKind::ValidationError => "validation_error",
            ToolFailureKind::PermissionDenied => "permission_denied",
            ToolFailureKind::PermissionRequired => "permission_required",
            ToolFailureKind::PermissionError => "permission_error",
            ToolFailureKind::ToolNotFound => "tool_not_found",
            ToolFailureKind::ExecutionError => "execution_error",
            ToolFailureKind::ExecutionPanic => "execution_panic",
        }
    }
}

/// Sanitized failure details safe to persist locally and summarize externally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizedToolFailure {
    pub argument_summary: String,
    pub error_summary: String,
    pub exit_code: Option<i32>,
}

/// A structured, sanitized record for one failed tool invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolFailureEvent {
    pub timestamp_unix_secs: u64,
    pub session_id: String,
    pub turn_id: u64,
    pub tool_call_id: String,
    pub tool_name: String,
    pub failure_kind: ToolFailureKind,
    pub argument_summary: String,
    pub error_summary: String,
    pub exit_code: Option<i32>,
    pub signature: String,
}

impl ToolFailureEvent {
    pub fn new(
        session_id: impl Into<String>,
        turn_id: u64,
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        failure_kind: ToolFailureKind,
        failure: SanitizedToolFailure,
    ) -> Self {
        let tool_name = tool_name.into();
        let signature =
            signature_for(&tool_name, failure_kind, &failure.argument_summary, &failure, 240);
        Self {
            timestamp_unix_secs: unix_timestamp(),
            session_id: session_id.into(),
            turn_id,
            tool_call_id: tool_call_id.into(),
            tool_name,
            failure_kind,
            argument_summary: failure.argument_summary,
            error_summary: failure.error_summary,
            exit_code: failure.exit_code,
            signature,
        }
    }
}

/// Sanitizes tool arguments and errors before diagnostics persistence.
#[derive(Debug, Clone)]
pub struct ToolFailureSanitizer {
    project_root: PathBuf,
    env_values: Vec<String>,
    pub home_dir: Option<PathBuf>,
}

impl ToolFailureSanitizer {
    pub fn new(project_root: PathBuf, env: HashMap<String, String>) -> Self {
        let mut env_values = env.into_values().filter(|value| value.len() >= 4).collect::<Vec<_>>();
        env_values.sort_by_key(|value| std::cmp::Reverse(value.len()));
        Self { project_root, env_values, home_dir: std::env::var_os("HOME").map(PathBuf::from) }
    }

    pub fn sanitize_failure(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        error: &str,
    ) -> SanitizedToolFailure {
        let argument_summary = self.sanitize_text(&argument_summary(tool_name, arguments));
        let error_summary = trim_chars(&self.sanitize_text(error), 2_000);
        SanitizedToolFailure {
            argument_summary,
            exit_code: extract_exit_code(error),
            error_summary,
        }
    }

    pub fn sanitize_text(&self, text: &str) -> String {
        let mut sanitized = text.to_string();
        sanitized = replace_path(&sanitized, &self.project_root, "<PROJECT>");
        if let Some(home_dir) = &self.home_dir {
            sanitized = replace_path(&sanitized, home_dir, "<HOME>");
        }
        sanitized = replace_path(&sanitized, Path::new("/tmp"), "<TMP>");
        for value in &self.env_values {
            sanitized = sanitized.replace(value, "<ENV_VALUE>");
        }
        sanitized = strip_url_queries(&sanitized);
        sanitized = redact_emails(&sanitized);
        sanitized = redact_secret_like_tokens(&sanitized);
        sanitized
    }
}

/// Receives sanitized diagnostics events from the executor.
#[async_trait]
pub trait ToolDiagnosticsSink: Send + Sync {
    async fn record(&self, event: ToolFailureEvent) -> Result<(), AgentError>;
}

#[derive(Debug, Default)]
pub struct NoopToolDiagnosticsSink;

#[async_trait]
impl ToolDiagnosticsSink for NoopToolDiagnosticsSink {
    async fn record(&self, _event: ToolFailureEvent) -> Result<(), AgentError> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct JsonlToolDiagnosticsSink {
    path: PathBuf,
    write_lock: Mutex<()>,
}

impl JsonlToolDiagnosticsSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), write_lock: Mutex::new(()) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl ToolDiagnosticsSink for JsonlToolDiagnosticsSink {
    async fn record(&self, event: ToolFailureEvent) -> Result<(), AgentError> {
        let _guard = self.write_lock.lock().await;
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| AgentError::Config(err.to_string()))?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .map_err(|err| AgentError::Config(err.to_string()))?;
        let line =
            serde_json::to_string(&event).map_err(|err| AgentError::Config(err.to_string()))?;
        file.write_all(line.as_bytes()).await.map_err(|err| AgentError::Config(err.to_string()))?;
        file.write_all(b"\n").await.map_err(|err| AgentError::Config(err.to_string()))?;
        Ok(())
    }
}

pub fn sanitized_event_for_failure(
    session_id: &str,
    turn_id: u64,
    tool_call_id: &str,
    tool_name: &str,
    failure_kind: ToolFailureKind,
    arguments: &serde_json::Value,
    error: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
) -> ToolFailureEvent {
    let sanitizer = ToolFailureSanitizer::new(cwd.to_path_buf(), env.clone());
    let failure = sanitizer.sanitize_failure(tool_name, arguments, error);
    ToolFailureEvent::new(session_id, turn_id, tool_call_id, tool_name, failure_kind, failure)
}

fn argument_summary(tool_name: &str, arguments: &serde_json::Value) -> String {
    let name = tool_name.to_ascii_lowercase();
    if (name == "bash" || name == "powershell")
        && let Some(command) = arguments.get("command").and_then(|value| value.as_str())
    {
        return summarize_command(command);
    }
    for key in ["file_path", "path", "pattern", "query", "url", "description"] {
        if let Some(value) = arguments.get(key).and_then(|value| value.as_str()) {
            return trim_chars(value, 240);
        }
    }
    trim_chars(&json!(arguments).to_string(), 240)
}

fn summarize_command(command: &str) -> String {
    let first_line = command.lines().next().unwrap_or(command).trim();
    let mut parts = first_line.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(first), Some(second))
            if matches!(first, "cargo" | "git" | "npm" | "pnpm" | "yarn") =>
        {
            format!("{first} {second}")
        }
        (Some(first), _) => first.to_string(),
        _ => String::new(),
    }
}

fn signature_for(
    tool_name: &str,
    failure_kind: ToolFailureKind,
    argument_summary: &str,
    failure: &SanitizedToolFailure,
    max_error_chars: usize,
) -> String {
    let exit = failure.exit_code.map(|code| format!(":exit={code}")).unwrap_or_default();
    let leading_error = failure.error_summary.lines().next().unwrap_or("").trim();
    format!(
        "{}:{}{}:{}:{}",
        tool_name,
        failure_kind.as_str(),
        exit,
        argument_summary,
        trim_chars(leading_error, max_error_chars)
    )
}

fn replace_path(text: &str, path: &Path, replacement: &str) -> String {
    let path = path.to_string_lossy();
    if path.is_empty() || path == "." {
        return text.to_string();
    }
    text.replace(path.as_ref(), replacement)
}

fn strip_url_queries(text: &str) -> String {
    text.split_whitespace()
        .map(|part| {
            if let Some(query_start) = part.find('?')
                && (part.starts_with("http://") || part.starts_with("https://"))
            {
                let (base, tail) = part.split_at(query_start);
                let suffix = tail
                    .find(|ch| [' ', '\n', '\t'].contains(&ch))
                    .map(|idx| &tail[idx..])
                    .unwrap_or("");
                return format!("{base}<QUERY>{suffix}");
            }
            part.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_emails(text: &str) -> String {
    text.split_whitespace()
        .map(|part| {
            let has_email_shape = part.contains('@') && part.rsplit_once('.').is_some();
            if has_email_shape { "<EMAIL>".to_string() } else { part.to_string() }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_secret_like_tokens(text: &str) -> String {
    text.split_whitespace()
        .map(|part| {
            let trimmed =
                part.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_');
            let lower = trimmed.to_ascii_lowercase();
            let secret_like = lower.starts_with("sk-")
                || lower.starts_with("ghp_")
                || lower.starts_with("github_pat_")
                || lower.starts_with("bearer");
            if secret_like { part.replace(trimmed, "<SECRET>") } else { part.to_string() }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_exit_code(error: &str) -> Option<i32> {
    let marker = "exit code Some(";
    let start = error.find(marker)? + marker.len();
    let rest = &error[start..];
    let end = rest.find(')')?;
    rest[..end].parse().ok()
}

fn trim_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let preview = text.chars().take(max_chars).collect::<String>();
    format!("{preview}<truncated>")
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn sanitizer_redacts_sensitive_values() {
        let mut sanitizer = ToolFailureSanitizer::new(
            PathBuf::from("/home/alice/project"),
            HashMap::from([("API_KEY".into(), "sk-secret-value".into())]),
        );
        sanitizer.home_dir = Some(PathBuf::from("/home/alice"));
        let text = "token sk-secret-value at /home/alice/project/src/main.rs user a@example.com https://example.com/path?secret=1";
        let sanitized = sanitizer.sanitize_text(text);
        assert!(!sanitized.contains("sk-secret-value"));
        assert!(!sanitized.contains("/home/alice"));
        assert!(!sanitized.contains("a@example.com"));
        assert!(!sanitized.contains("secret=1"));
        assert!(sanitized.contains("<ENV_VALUE>"));
        assert!(sanitized.contains("<PROJECT>"));
        assert!(sanitized.contains("<EMAIL>"));
    }

    #[test]
    fn event_signature_is_stable_for_sanitized_failures() {
        let event = ToolFailureEvent::new(
            "s1",
            2,
            "call-1",
            "Bash",
            ToolFailureKind::ExecutionError,
            SanitizedToolFailure {
                argument_summary: "cargo test".into(),
                error_summary: "Command failed with exit code Some(101)".into(),
                exit_code: Some(101),
            },
        );
        assert_eq!(
            event.signature,
            "Bash:execution_error:exit=101:cargo test:Command failed with exit code Some(101)"
        );
    }

    #[test]
    fn powershell_arguments_are_summarized_like_shell_commands() {
        let summary = argument_summary(
            "PowerShell",
            &json!({ "command": "Get-ChildItem -Force C:\\Users\\alice" }),
        );

        assert_eq!(summary, "Get-ChildItem");
    }
}
