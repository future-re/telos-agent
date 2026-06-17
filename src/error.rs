//! Unified error type for the crate.
//!
//! All public APIs surface failures through [`AgentError`]. Each variant
//! identifies a distinct failure class so callers can pattern-match instead of
//! parsing error strings.

use thiserror::Error;

/// All error conditions surfaced by the agent runtime.
#[derive(Debug, Error)]
pub enum AgentError {
    /// The model provider (HTTP transport, API contract, deserialisation, …) failed.
    #[error("provider error: {0}")]
    Provider(String),
    /// A misconfigured session or backend (missing env var, unwritable storage dir, …).
    #[error("configuration error: {0}")]
    Config(String),
    /// Tool input failed schema/business-rule validation before execution.
    #[error("validation error: {0}")]
    Validation(String),
    /// The permission engine or a tool refused to execute the call.
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// The assistant requested a tool that isn't in the registry.
    #[error("tool not found: {0}")]
    ToolNotFound(String),
    /// A tool ran but reported a runtime failure.
    #[error("tool `{tool}` failed: {message}")]
    ToolExecution { tool: String, message: String },
    /// The turn loop exceeded [`AgentConfig::max_iterations`](crate::AgentConfig::max_iterations).
    #[error("maximum tool iterations reached: {0}")]
    MaxIterations(usize),
    /// Provider retries exhausted — every attempt failed.
    #[error("provider retries exhausted after {attempts} attempts: {last_error}")]
    ProviderRetriesExhausted { attempts: usize, last_error: String },
    /// The turn was cancelled via [`CancellationToken`].
    #[error("cancelled")]
    Cancelled,
}

impl AgentError {
    /// Whether the error is transient and worth retrying.
    ///
    /// Network errors and server-side 429/5xx responses are retryable;
    /// configuration errors, validation failures, and permission denials are not.
    pub fn is_retryable(&self) -> bool {
        match self {
            AgentError::Provider(msg) => {
                let lower = msg.to_lowercase();
                lower.contains("429")
                    || lower.contains("500")
                    || lower.contains("502")
                    || lower.contains("503")
                    || lower.contains("504")
                    || lower.contains("timeout")
                    || lower.contains("timed out")
                    || lower.contains("connection")
                    || lower.contains("reset")
                    || lower.contains("refused")
                    || lower.contains("eof")
                    || lower.contains("broken pipe")
                    || lower.contains("rate limit")
            }
            _ => false,
        }
    }
}
