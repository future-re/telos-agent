//! Unified error type for the crate.
//!
//! All public APIs surface failures through [`AgentError`]. Each variant
//! identifies a distinct failure class so callers can pattern-match instead of
//! parsing error strings.

use serde::Serialize;
use thiserror::Error;

/// Structured failure categories for provider (HTTP / API / stream) errors.
///
/// Carrying the HTTP status code lets the runtime decide whether an error is
/// transient and worth retrying without parsing free-text messages.
#[derive(Debug, Error, Clone, Serialize)]
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
#[derive(Debug, Error, Clone, Serialize)]
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
    /// The turn was cancelled via [`CancellationState`](crate::CancellationState).
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

    /// Whether the error indicates the context window was exceeded.
    ///
    /// HTTP 400 with a message about context/token length means the request
    /// was too large for the model. This is NOT retryable in the normal sense
    /// (repeating the same request will fail), but it IS recoverable via
    /// compaction: summarise history, then retry with fewer tokens.
    pub fn is_context_too_long(&self) -> bool {
        match self {
            AgentError::Provider(ProviderError::Http { status, message }) if *status == 400 => {
                let lower = message.to_lowercase();
                lower.contains("context")
                    || lower.contains("token")
                    || lower.contains("too long")
                    || lower.contains("too large")
                    || lower.contains("exceeds")
                    || lower.contains("maximum")
                    || lower.contains("length")
            }
            AgentError::Provider(ProviderError::Api(message)) => {
                let lower = message.to_lowercase();
                lower.contains("context") || lower.contains("token") || lower.contains("too long")
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_429_is_retryable() {
        let err = AgentError::Provider(ProviderError::Http {
            status: 429,
            message: "rate limited".into(),
        });
        assert!(err.is_retryable());
    }

    #[test]
    fn http_5xx_is_retryable() {
        for status in [500, 502, 503, 504] {
            let err = AgentError::Provider(ProviderError::Http {
                status,
                message: "server error".into(),
            });
            assert!(err.is_retryable(), "status {status} should be retryable");
        }
    }

    #[test]
    fn http_4xx_other_than_429_is_not_retryable() {
        for status in [400, 401, 403, 404, 422] {
            let err = AgentError::Provider(ProviderError::Http {
                status,
                message: "client error".into(),
            });
            assert!(!err.is_retryable(), "status {status} should not be retryable");
        }
    }

    #[test]
    fn network_and_timeout_errors_are_retryable() {
        assert!(AgentError::Provider(ProviderError::Network("reset".into())).is_retryable());
        assert!(AgentError::Provider(ProviderError::Timeout).is_retryable());
    }

    #[test]
    fn api_invalid_response_stream_ended_are_not_retryable() {
        assert!(!AgentError::Provider(ProviderError::Api("invalid key".into())).is_retryable());
        assert!(
            !AgentError::Provider(ProviderError::InvalidResponse("bad json".into())).is_retryable()
        );
        assert!(!AgentError::Provider(ProviderError::StreamEnded).is_retryable());
    }

    #[test]
    fn non_provider_errors_are_not_retryable() {
        assert!(!AgentError::Config("bad".into()).is_retryable());
        assert!(!AgentError::Validation("bad".into()).is_retryable());
        assert!(!AgentError::PermissionDenied("no".into()).is_retryable());
        assert!(!AgentError::ToolNotFound("x".into()).is_retryable());
    }
}
