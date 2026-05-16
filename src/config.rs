use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::compaction::CompactionStrategy;
use crate::hooks::HookRegistry;
use crate::storage::Storage;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub system_prompt: Option<String>,
    pub max_iterations: usize,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub max_tool_result_chars: usize,
    pub hooks: Arc<HookRegistry>,
    pub storage: Option<Arc<dyn Storage>>,
    pub compaction: Option<Arc<dyn CompactionStrategy>>,
    pub permission_engine: Option<crate::permissions::PermissionEngine>,
    pub tool_concurrency_limit: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBudget {
    pub max_tokens: usize,
    pub compact_at_tokens: usize,
}

impl TokenBudget {
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            compact_at_tokens: ((max_tokens as f64) * 0.8).ceil() as usize,
        }
    }
}
