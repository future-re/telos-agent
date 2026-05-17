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
}
