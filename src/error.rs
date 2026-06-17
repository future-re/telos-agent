//! Unified error type for the crate.
//!
//! All public APIs surface failures through [`AgentError`]. Each variant
//! identifies a distinct failure class so callers can pattern-match instead of
//! parsing error strings.

use thiserror::Error;

/// Structured failure categories for provider (HTTP / API / stream) errors.
///
/// Carrying the HTTP status code lets the runtime decide whether an error is
/// transient and worth retrying without parsing free-text messages.
#[derive(Debug, Error, Clone)]
pub enum ProviderError {
    /// An HTTP response with a non-success status code.
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String },
    /// A low-level network error (connection reset, refused, EOF, etc.).
    #[error("network error: {0}")]
    Network(String),
    /// A provider API-level error response.
    #[error("API error: {0}")]
    Api(String),
    /// The request timed out before completing.
    #[error("timeout")]
    Timeout,
    /// The provider stream ended before a complete message was received.
    #[error("stream ended unexpectedly")]
    StreamEnded,
    /// The provider returned a response that could not be parsed.
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    /// Any other provider failure that does not fit the categories above.
    #[error("{0}")]
    Other(String),
}

/// All error conditions surfaced by the agent runtime.
#[derive(Debug, Error)]
pub enum AgentError {
    /// The model provider (HTTP transport, API contract, deserialisation, …) failed.
    #[error("provider error: {0}")]
    Provider(ProviderError),
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
            AgentError::Provider(err) => match err {
                ProviderError::Http { status, .. } => {
                    *status == 429 || (500..=599).contains(status)
                }
                ProviderError::Network(_) | ProviderError::Timeout => true,
                _ => false,
            },
            _ => false,
        }
    }
}
