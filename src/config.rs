//! Configuration types for an [`AgentSession`](crate::AgentSession).
//!
//! [`AgentConfig`] aggregates everything a session needs: model behaviour
//! (system prompt, iteration cap), execution environment (cwd, env), and the
//! pluggable extension points (hooks, storage, compaction, permissions).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

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
    /// Maximum number of concurrency-safe tools to run in parallel within a single batch.
    pub tool_concurrency_limit: usize,
    /// Optional token budget that triggers proactive compaction.
    pub token_budget: Option<TokenBudget>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: None,
            max_iterations: 8,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            env: std::env::vars().collect(),
            max_tool_result_chars: usize::MAX,
            hooks: Arc::new(HookRegistry::new()),
            storage: None,
            compaction: None,
            permission_engine: None,
            tool_concurrency_limit: 10,
            token_budget: None,
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
        Self {
            max_tokens,
            compact_at_tokens: ((max_tokens as f64) * 0.8).ceil() as usize,
        }
    }
}
