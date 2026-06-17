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
use crate::hooks::HookRegistry;
use crate::storage::Storage;

/// Configuration for an [`AgentSession`](crate::AgentSession).
///
/// Build with [`Default::default`] and override fields as needed. All fields
/// are public so callers don't need a builder for simple cases.
#[derive(Debug, Clone)]
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

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: None,
            max_iterations: 8,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            // Start with an empty environment for shell tools. Callers that need
            // specific variables can add them explicitly; inheriting the entire
            // process environment risks leaking secrets to arbitrary tool calls.
            env: HashMap::new(),
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
        Self {
            max_retries: 3,
            base_delay_ms: 500,
            max_delay_ms: 10_000,
        }
    }
}

impl RetryConfig {
    /// No retries at all — a convenient sentinel that can replace `Option<RetryConfig>`.
    pub const NONE: Self = Self {
        max_retries: 0,
        base_delay_ms: 0,
        max_delay_ms: 0,
    };

    /// Whether the error is retryable and we still have attempts left.
    pub fn should_retry(&self, error: &crate::error::AgentError, attempts: usize) -> bool {
        attempts < self.max_retries && error.is_retryable()
    }

    /// Exponential backoff delay for the given attempt (1-indexed).
    pub fn delay_for(&self, attempt: usize) -> std::time::Duration {
        let delay = self.base_delay_ms.saturating_mul(1u64 << (attempt.saturating_sub(1)));
        let capped = delay.min(self.max_delay_ms);
        std::time::Duration::from_millis(capped)
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
    fn default_env_is_empty() {
        let config = AgentConfig::default();
        assert!(config.env.is_empty());
    }
}
