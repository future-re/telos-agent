//! Configuration types for an [`AgentSession`](crate::AgentSession).
//!
//! [`AgentConfig`] aggregates everything a session needs: model behaviour
//! (system prompt, iteration cap), execution environment (cwd, env), and the
//! pluggable extension points (hooks, storage, compaction, permissions).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::approval::ApprovalHandler;
use crate::compaction::CompactionStrategy;
use crate::error::AgentError;
use crate::hooks::HookRegistry;
use crate::storage::Storage;

/// Configuration for an [`AgentSession`](crate::AgentSession).
///
/// Build with [`Default::default`] and override fields as needed. All fields
/// are public so callers don't need a builder for simple cases.
#[derive(Clone)]
pub struct AgentConfig {
    /// Optional system prompt prepended to every conversation.
    pub system_prompt: Option<String>,
    /// Maximum number of model ⇄ tool round-trips per turn before the loop aborts
    /// with [`AgentError::MaxIterations`](crate::AgentError::MaxIterations).
    pub max_iterations: usize,
    /// Working directory used as the root for filesystem tools and shell commands.
    pub cwd: PathBuf,
    /// Environment variables exposed to shell-based tools.
    pub env: HashMap<String, String>,
    /// Hard cap on the character length of any individual tool result. Anything
    /// longer is replaced with a truncated preview to protect the context window.
    pub max_tool_result_chars: usize,
    /// Registry of [`Hook`](crate::Hook)s invoked at well-known turn phases.
    pub hooks: Arc<HookRegistry>,
    /// Optional persistent backing store for session messages.
    pub storage: Option<Arc<dyn Storage>>,
    /// Optional history-level compaction strategy (summarisation, etc.).
    pub compaction: Option<Arc<dyn CompactionStrategy>>,
    /// Optional rule-based permission engine consulted before every tool call.
    pub permission_engine: Option<crate::permissions::PermissionEngine>,
    /// Optional handler invoked when a tool call requires explicit human approval.
    pub approval_handler: Option<Arc<dyn ApprovalHandler>>,
    /// Maximum number of concurrency-safe tools to run in parallel within a single batch.
    pub tool_concurrency_limit: usize,
    /// Optional token budget that triggers proactive compaction.
    pub token_budget: Option<TokenBudget>,
    /// Retry configuration for transient provider failures.
    /// When `None`, provider calls are not retried.
    pub retry: RetryConfig,
    /// Whether to automatically validate tool arguments against the tool's
    /// `input_schema` after the tool's own `validate` method runs.
    pub auto_validate_schema: bool,
    /// Shared cancellation flag. Set to `true` from another task to cancel the
    /// running turn as soon as the next checkpoint is reached.
    pub cancelled: Arc<AtomicBool>,
    /// Per-tool execution timeout in milliseconds. When set, any tool that runs
    /// longer than this will be interrupted and its result will be an
    /// `is_error: true` timeout so the model can recover.
    pub tool_timeout_ms: Option<u64>,
    /// Maximum number of bytes the built-in file tools will read from a single
    /// file. Files larger than this are rejected to avoid OOMing the agent.
    pub max_file_read_bytes: usize,
}

impl std::fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Redact `env` values because they may contain secrets; keys are kept
        // for debugging. Other fields are considered non-sensitive.
        let env_keys: Vec<&String> = self.env.keys().collect();
        f.debug_struct("AgentConfig")
            .field("system_prompt", &self.system_prompt)
            .field("max_iterations", &self.max_iterations)
            .field("cwd", &self.cwd)
            .field("env", &format!("{} keys: [REDACTED]", env_keys.len()))
            .field("max_tool_result_chars", &self.max_tool_result_chars)
            .field("hooks", &self.hooks)
            .field("storage", &self.storage)
            .field("compaction", &self.compaction)
            .field("permission_engine", &self.permission_engine)
            .field("approval_handler", &self.approval_handler)
            .field("tool_concurrency_limit", &self.tool_concurrency_limit)
            .field("token_budget", &self.token_budget)
            .field("retry", &self.retry)
            .field("auto_validate_schema", &self.auto_validate_schema)
            .field("cancelled", &self.cancelled)
            .field("tool_timeout_ms", &self.tool_timeout_ms)
            .field("max_file_read_bytes", &self.max_file_read_bytes)
            .finish()
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: None,
            max_iterations: 8,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            // Start with a minimal environment for shell tools. Callers that need
            // specific variables can add them explicitly; inheriting the entire
            // process environment risks leaking secrets to arbitrary tool calls.
            // A default PATH keeps simple safe commands (`ls`, `cat`, etc.) working.
            env: HashMap::from([("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into())]),
            max_tool_result_chars: usize::MAX,
            hooks: Arc::new(HookRegistry::new()),
            storage: None,
            compaction: None,
            permission_engine: None,
            approval_handler: None,
            tool_concurrency_limit: 10,
            token_budget: None,
            retry: RetryConfig::default(),
            auto_validate_schema: true,
            cancelled: Arc::new(AtomicBool::new(false)),
            tool_timeout_ms: None,
            max_file_read_bytes: 50 * 1024 * 1024,
        }
    }
}

/// Token budget that triggers automatic compaction before the hard limit is hit.
///
/// The runtime estimates request size locally (via
/// [`ModelProvider::estimate_tokens`](crate::ModelProvider::estimate_tokens))
/// and compacts when usage exceeds `compact_at_tokens`. If a request still
/// exceeds `max_tokens` after compaction, the turn ends to avoid an API
/// rejection.
///
/// `compact_at_tokens` defaults to 80% of `max_tokens`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBudget {
    /// Hard ceiling — the turn ends before the model is called if exceeded.
    pub max_tokens: usize,
    /// Soft ceiling — proactively compact when crossed.
    pub compact_at_tokens: usize,
}

impl TokenBudget {
    /// Build a budget with `compact_at_tokens` set to 80% of `max_tokens`.
    pub fn new(max_tokens: usize) -> Self {
        Self { max_tokens, compact_at_tokens: ((max_tokens as f64) * 0.8).ceil() as usize }
    }
}

/// Retry behaviour for transient provider failures.
///
/// When a provider call fails with a retryable error (network errors, 429, 5xx),
/// the runtime will retry with exponential backoff up to `max_retries` times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (0 = no retries).
    pub max_retries: usize,
    /// Initial backoff delay in milliseconds.
    pub base_delay_ms: u64,
    /// Maximum backoff delay in milliseconds.
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self { max_retries: 3, base_delay_ms: 500, max_delay_ms: 10_000 }
    }
}

impl RetryConfig {
    /// No retries at all — a convenient sentinel that can replace `Option<RetryConfig>`.
    pub const NONE: Self = Self { max_retries: 0, base_delay_ms: 0, max_delay_ms: 0 };

    /// Whether the error is retryable and we still have attempts left.
    ///
    /// `attempts` is the number of provider calls already made (1-indexed).
    /// Retrying is allowed while `attempts <= max_retries`, so `max_retries: 3`
    /// yields up to three retry attempts after the initial call.
    pub fn should_retry(&self, error: &crate::error::AgentError, attempts: usize) -> bool {
        attempts <= self.max_retries && error.is_retryable()
    }

    /// Exponential backoff delay for the given attempt (1-indexed).
    pub fn delay_for(&self, attempt: usize) -> std::time::Duration {
        // Cap the shift to avoid undefined behaviour / overflow when attempt is
        // very large (shifting a u64 by >= 64 bits is UB in Rust).
        let shift = attempt.saturating_sub(1).min(63);
        let delay = self.base_delay_ms.saturating_mul(1u64 << shift);
        let capped = delay.min(self.max_delay_ms);
        std::time::Duration::from_millis(capped)
    }
}

impl AgentConfig {
    /// Validate configuration values and return a structured error for any
    /// dangerous or nonsensical combination.
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.max_iterations == 0 {
            return Err(AgentError::Config("max_iterations must be greater than 0".into()));
        }
        if self.tool_concurrency_limit == 0 {
            return Err(AgentError::Config("tool_concurrency_limit must be greater than 0".into()));
        }
        if self.max_file_read_bytes == 0 {
            return Err(AgentError::Config("max_file_read_bytes must be greater than 0".into()));
        }
        if self.max_tool_result_chars == 0 {
            return Err(AgentError::Config("max_tool_result_chars must be greater than 0".into()));
        }
        if let Some(budget) = self.token_budget {
            if budget.max_tokens == 0 {
                return Err(AgentError::Config(
                    "token_budget.max_tokens must be greater than 0".into(),
                ));
            }
            if budget.compact_at_tokens == 0 {
                return Err(AgentError::Config(
                    "token_budget.compact_at_tokens must be greater than 0".into(),
                ));
            }
            if budget.compact_at_tokens > budget.max_tokens {
                return Err(AgentError::Config(
                    "token_budget.compact_at_tokens cannot exceed max_tokens".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_budget_new_sets_compact_at_80_percent() {
        let budget = TokenBudget::new(1000);
        assert_eq!(budget.max_tokens, 1000);
        assert_eq!(budget.compact_at_tokens, 800);
    }

    #[test]
    fn token_budget_rounds_up() {
        let budget = TokenBudget::new(100);
        // 80% of 100 = 80, ceil(80) = 80
        assert_eq!(budget.compact_at_tokens, 80);
        let budget2 = TokenBudget::new(101);
        // 80% of 101 = 80.8, ceil = 81
        assert_eq!(budget2.compact_at_tokens, 81);
    }

    #[test]
    fn default_env_contains_only_path() {
        let config = AgentConfig::default();
        assert_eq!(config.env.len(), 1);
        assert_eq!(config.env.get("PATH"), Some(&"/usr/local/bin:/usr/bin:/bin".into()));
    }

    #[test]
    fn default_config_validates() {
        let config = AgentConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn zero_max_iterations_invalid() {
        let config = AgentConfig { max_iterations: 0, ..AgentConfig::default() };
        assert!(config.validate().is_err());
    }

    #[test]
    fn token_budget_compact_exceeding_max_invalid() {
        let config = AgentConfig {
            token_budget: Some(TokenBudget { max_tokens: 100, compact_at_tokens: 200 }),
            ..AgentConfig::default()
        };
        assert!(config.validate().is_err());
    }
}
