//! Configuration types for an [`AgentSession`](crate::AgentSession).
//!
//! [`AgentConfig`] aggregates everything a session needs: model behaviour
//! (system prompt, optional iteration cap), execution environment (cwd, env), and the
//! pluggable extension points (hooks, storage, compaction, permissions).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::approval::ApprovalHandler;
use crate::compaction::{CompactionStrategy, SummaryCompaction};
use crate::diagnostics::ToolDiagnosticsSink;
use crate::error::AgentError;
use crate::hooks::HookRegistry;
use crate::prompt::PromptProfile;
use crate::storage::Storage;

/// Return the small set of host environment variables needed for tools to
/// launch platform-native child processes without inheriting the full process
/// environment.
pub fn platform_base_env() -> HashMap<String, String> {
    let mut env = HashMap::new();
    for key in platform_base_env_keys() {
        if let Some((actual_key, value)) =
            std::env::vars().find(|(name, _)| name.eq_ignore_ascii_case(key))
        {
            env.insert(actual_key, value);
        }
    }
    env
}

#[cfg(windows)]
fn platform_base_env_keys() -> &'static [&'static str] {
    &[
        "Path",
        "SystemRoot",
        "WINDIR",
        "USERPROFILE",
        "TEMP",
        "TMP",
        "APPDATA",
        "LOCALAPPDATA",
        "ComSpec",
        "PATHEXT",
        "PSModulePath",
    ]
}

#[cfg(not(windows))]
fn platform_base_env_keys() -> &'static [&'static str] {
    &["PATH", "HOME"]
}

/// Shared cancellation state for one agent runtime.
///
/// The atomic flag preserves cheap synchronous checks, while the notify handle
/// wakes async waits that would otherwise remain parked until a provider or
/// tool produces another event.
#[derive(Clone, Debug)]
pub struct CancellationState {
    flag: Arc<AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl CancellationState {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn from_flag(flag: Arc<AtomicBool>) -> Self {
        Self { flag, notify: Arc::new(tokio::sync::Notify::new()) }
    }

    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.flag)
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
        self.notify.notify_waiters();
    }

    pub fn reset(&self) {
        self.flag.store(false, Ordering::Relaxed);
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        loop {
            self.notify.notified().await;
            if self.is_cancelled() {
                return;
            }
        }
    }
}

impl Default for CancellationState {
    fn default() -> Self {
        Self::new()
    }
}

/// Workflow path — the agent tailors its runtime behaviour and prompt guidance
/// to match the expected scope and risk of the task.
///
/// | Path      | Typical task                                     |
/// |-----------|--------------------------------------------------|
/// | `Fast`    | Single-file fix, clear bug, small config change  |
/// | `Standard`| Multi-file change, local restructure             |
/// | `Heavy`   | New feature, cross-module refactor, 3+ steps     |
///
/// The path adjusts iteration caps, timeouts, and concurrency defaults. It
/// also injects matching behavioural guidance into the system prompt so the
/// model knows whether to work directly or follow a fuller plan cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskPath {
    /// Single-file changes, clear bugs, small config — execute directly.
    Fast,
    /// Multi-file changes, local restructures — map context, verify incrementally.
    #[default]
    Standard,
    /// New features, cross-module refactors — design, plan, execute in phases.
    Heavy,
}

/// Configuration for an [`AgentSession`](crate::AgentSession).
///
/// Build with [`Default::default`] and override fields as needed. All fields
/// are public so callers don't need a builder for simple cases.
#[derive(Clone)]
pub struct AgentConfig {
    /// Workflow path classification for the current task. Influences iteration
    /// budget, timeout, concurrency, and the system-prompt guidance injected at
    /// turn time.
    pub path: TaskPath,
    /// Optional base instruction appended to the identity section of the
    /// system prompt. For full control, use `prompt_assembly` instead.
    pub base_system_prompt: Option<String>,
    /// Optional pre-built prompt assembly. When set, the runtime uses this
    /// instead of constructing the prompt from `base_system_prompt` alone.
    pub prompt_assembly: Option<std::sync::Arc<crate::prompt::PromptAssembly>>,
    /// Controls how much built-in prompt guidance is injected when the runtime
    /// constructs the default prompt assembly.
    pub prompt_profile: PromptProfile,
    /// Optional maximum number of model ⇄ tool round-trips per turn before the
    /// loop aborts with [`AgentError::MaxIterations`].
    ///
    /// `None` means the model/tool loop is allowed to continue until the model
    /// finishes, the turn is cancelled, or another runtime limit is reached.
    pub max_iterations: Option<usize>,
    /// Working directory used as the root for filesystem tools and shell commands.
    pub cwd: PathBuf,
    /// Environment variables exposed to shell-based tools.
    pub env: HashMap<String, String>,
    /// Hard cap on the character length of any individual tool result. Anything
    /// longer is replaced with a truncated preview to protect the context window.
    pub max_tool_result_chars: usize,
    /// Hard cap on the aggregate character length of all tool results within a
    /// single message. When N parallel tool calls collectively exceed this, the
    /// largest results are truncated to fit. Prevents a turn with many tool
    /// calls from flooding the context window even when each result individually
    /// stays under `max_tool_result_chars`.
    pub max_message_tool_results_chars: usize,
    /// Registry of [`Hook`](crate::Hook)s invoked at well-known turn phases.
    pub hooks: Arc<HookRegistry>,
    /// Optional persistent backing store for session messages.
    pub storage: Option<Arc<dyn Storage>>,
    /// Optional sink for sanitized tool failure diagnostics.
    pub tool_diagnostics: Option<Arc<dyn ToolDiagnosticsSink>>,
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
    /// Shared cancellation state. Hosts call `cancel()` to interrupt a running
    /// turn and `reset()` before starting the next turn.
    pub cancellation: CancellationState,
    /// Per-tool execution timeout in milliseconds. When set, any tool that runs
    /// longer than this will be interrupted and its result will be an
    /// `is_error: true` timeout so the model can recover.
    pub tool_timeout_ms: Option<u64>,
    /// Maximum number of bytes the built-in file tools will read from a single
    /// file. Files larger than this are rejected to avoid OOMing the agent.
    pub max_file_read_bytes: usize,
    /// Optional skill registry. When set, the Skill tool and the default prompt
    /// assembly can use registered skills (including bundled Superpowers skills).
    pub skill_registry: Option<Arc<crate::skills::SkillRegistry>>,
    /// Optional plugin registry. When set, enabled plugins' components are
    /// applied to tool/hook/skill registries at session startup.
    pub plugin_registry: Option<Arc<crate::plugin::PluginRegistry>>,
    /// Optional task manager used by long-running tools such as background
    /// subagents to publish lifecycle state and output.
    pub task_manager: Option<Arc<crate::tasks::TaskManager>>,
    /// Optional memory injector for dynamic per-turn memory scoring.
    /// When set, memories are scored against the user's current input
    /// and injected as system reminders before the first provider call
    /// of each turn.
    pub memory_injector: Option<Arc<crate::runtime::MemoryInjector>>,
    /// Optional skill injector for dynamic per-turn skill discovery.
    /// When set, top matching skills are injected as system reminders
    /// before the first provider call of each turn.
    pub skill_injector: Option<Arc<crate::runtime::SkillInjector>>,
    /// Optional MCP manager. When set, MCP server tools are bridged into the
    /// tool registry and an MCP tools section is added to the prompt assembly.
    pub mcp_manager: Option<Arc<crate::mcp::McpManager>>,
    /// Optional configuration for the bidirectional HTTP event channel.
    /// When set and `enabled` is `true`, the session starts an embedded HTTP
    /// server on the configured address, accepting external event injection
    /// (`POST /inject`) and streaming TurnEvents via SSE (`GET /events`).
    pub event_channel: Option<crate::event_channel::EventChannelConfig>,
}

impl std::fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Redact `env` values because they may contain secrets; keys are kept
        // for debugging. Other fields are considered non-sensitive.
        let env_keys: Vec<&String> = self.env.keys().collect();
        f.debug_struct("AgentConfig")
            .field("path", &self.path)
            .field("base_system_prompt", &self.base_system_prompt)
            .field("prompt_assembly", &self.prompt_assembly.as_ref().map(|_| "<set>"))
            .field("prompt_profile", &self.prompt_profile)
            .field("max_iterations", &self.max_iterations)
            .field("cwd", &self.cwd)
            .field("env", &format!("{} keys: [REDACTED]", env_keys.len()))
            .field("max_tool_result_chars", &self.max_tool_result_chars)
            .field("max_message_tool_results_chars", &self.max_message_tool_results_chars)
            .field("hooks", &self.hooks)
            .field("storage", &self.storage)
            .field("tool_diagnostics", &self.tool_diagnostics.as_ref().map(|_| "<set>"))
            .field("compaction", &self.compaction)
            .field("permission_engine", &self.permission_engine)
            .field("approval_handler", &self.approval_handler)
            .field("tool_concurrency_limit", &self.tool_concurrency_limit)
            .field("token_budget", &self.token_budget)
            .field("retry", &self.retry)
            .field("auto_validate_schema", &self.auto_validate_schema)
            .field("cancellation", &self.cancellation)
            .field("tool_timeout_ms", &self.tool_timeout_ms)
            .field("max_file_read_bytes", &self.max_file_read_bytes)
            .field("skill_registry", &self.skill_registry.as_ref().map(|_| "<set>"))
            .field("plugin_registry", &self.plugin_registry.as_ref().map(|_| "<set>"))
            .field("task_manager", &self.task_manager.as_ref().map(|_| "<set>"))
            .field("memory_injector", &self.memory_injector.as_ref().map(|_| "<set>"))
            .field("skill_injector", &self.skill_injector.as_ref().map(|_| "<set>"))
            .field("mcp_manager", &self.mcp_manager.as_ref().map(|_| "<set>"))
            .field("event_channel", &self.event_channel)
            .finish()
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            path: TaskPath::default(),
            base_system_prompt: None,
            prompt_assembly: None,
            prompt_profile: PromptProfile::default(),
            max_iterations: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            // Start with a minimal platform environment for shell tools. Callers
            // that need specific variables can add them explicitly; inheriting
            // the entire process environment risks leaking secrets to arbitrary
            // tool calls.
            env: platform_base_env(),
            max_tool_result_chars: 50_000,
            max_message_tool_results_chars: 300_000,
            hooks: Arc::new(HookRegistry::new()),
            storage: None,
            tool_diagnostics: None,
            compaction: Some(Arc::new(SummaryCompaction { max_tokens: 800_000, keep_recent: 12 })),
            permission_engine: None,
            approval_handler: None,
            tool_concurrency_limit: 10,
            token_budget: None,
            retry: RetryConfig::default(),
            auto_validate_schema: true,
            cancellation: CancellationState::new(),
            tool_timeout_ms: None,
            max_file_read_bytes: 50 * 1024 * 1024,
            skill_registry: None,
            plugin_registry: None,
            task_manager: None,
            memory_injector: None,
            skill_injector: None,
            mcp_manager: None,
            event_channel: None,
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
    /// Request cancellation and wake any runtime await that is listening for it.
    pub fn request_cancel(&self) {
        self.cancellation.cancel();
    }

    /// Clear a previous cancellation request before starting a new turn.
    pub fn reset_cancel(&self) {
        self.cancellation.reset();
    }

    /// Validate configuration values and return a structured error for any
    /// dangerous or nonsensical combination.
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.max_iterations == Some(0) {
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

    /// Replace `base_system_prompt` with the default modular prompt assembly.
    ///
    /// This is the recommended way to use telos-agent's built-in default
    /// prompt sections without manually wiring each section.
    pub fn with_default_prompt_assembly(
        mut self,
        tools: Arc<crate::tool::ToolRegistry>,
    ) -> Result<Self, AgentError> {
        let assembly = crate::prompt::default_coding_assembly_for_profile(
            tools,
            self.cwd.clone(),
            self.skill_registry.clone(),
            self.path,
            self.prompt_profile,
        );
        self.base_system_prompt = None;
        self.prompt_assembly = Some(Arc::new(assembly));
        Ok(self)
    }

    /// Apply path-appropriate defaults for timeouts, token budgets, and
    /// concurrency. Call after setting custom values if you want the path to
    /// override them.
    ///
    /// | Knob                   | Fast      | Standard  | Heavy     |
    /// |------------------------|-----------|-----------|-----------|
    /// | `max_iterations`       | None      | None      | None      |
    /// | `tool_concurrency_limit`| 10       | 10        | 5         |
    /// | `token_budget`         | None      | None      | 308k      |
    /// | `tool_timeout_ms`      | 30_000    | None      | 60_000    |
    pub fn with_path(mut self, path: TaskPath) -> Self {
        self.path = path;
        match path {
            TaskPath::Fast => {
                self.max_iterations = None;
                self.tool_timeout_ms = Some(30_000);
                // Fast tasks shouldn't need compaction — keep budget off.
                self.token_budget = None;
            }
            TaskPath::Standard => {
                self.max_iterations = None;
                self.tool_timeout_ms = None;
                // 1M context window — compact early to leave headroom for output.
                self.token_budget = Some(TokenBudget::new(900_000));
            }
            TaskPath::Heavy => {
                self.max_iterations = None;
                self.tool_timeout_ms = Some(60_000);
                self.token_budget = Some(TokenBudget::new(950_000));
                self.tool_concurrency_limit = 5;
            }
        }
        self
    }

    /// Load bundled skills into a fresh skill registry attached to this config.
    pub fn with_bundled_skills(mut self) -> Self {
        let mut registry = crate::skills::SkillRegistry::new();
        registry.load_bundled_skills();
        let registry = Arc::new(registry);
        self.skill_injector =
            Some(Arc::new(crate::runtime::SkillInjector::new(Arc::clone(&registry))));
        self.skill_registry = Some(registry);
        self
    }

    /// Apply plugin components into the agent registries.
    ///
    /// Call this before creating an [`AgentSession`](crate::AgentSession) to
    /// populate tools/hooks/skills/mcp/prompt with the content of any enabled
    /// plugins. Always returns the registries (even on error) so callers can
    /// continue with degraded state.
    pub fn apply_plugins(
        &self,
        mut tools: crate::tool::ToolRegistry,
        mut hooks: crate::hooks::HookRegistry,
        mut skills: crate::skills::SkillRegistry,
        mut mcp: crate::mcp::McpManager,
        mut prompt: crate::prompt::PromptAssembly,
    ) -> (
        crate::tool::ToolRegistry,
        crate::hooks::HookRegistry,
        crate::skills::SkillRegistry,
        crate::mcp::McpManager,
        crate::prompt::PromptAssembly,
        Result<(), Vec<crate::plugin::PluginError>>,
    ) {
        let result = if let Some(registry) = &self.plugin_registry {
            registry.apply(&mut tools, &mut hooks, &mut skills, &mut mcp, &mut prompt)
        } else {
            Ok(())
        };
        (tools, hooks, skills, mcp, prompt, result)
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
    #[cfg(not(windows))]
    fn default_env_contains_unix_runtime_env() {
        let config = AgentConfig::default();
        assert!(config.env.contains_key("PATH"));
        assert_eq!(config.env.get("HOME"), std::env::var("HOME").ok().as_ref());
    }

    #[test]
    #[cfg(windows)]
    fn default_env_preserves_windows_runtime_env() {
        let config = AgentConfig::default();
        let expected = [
            "Path",
            "SystemRoot",
            "WINDIR",
            "USERPROFILE",
            "TEMP",
            "TMP",
            "APPDATA",
            "LOCALAPPDATA",
            "ComSpec",
            "PATHEXT",
            "PSModulePath",
        ];

        let mut present = 0;
        for key in expected {
            if let Some((actual_key, actual_value)) =
                std::env::vars().find(|(name, _)| name.eq_ignore_ascii_case(key))
            {
                present += 1;
                assert_eq!(
                    config
                        .env
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case(&actual_key))
                        .map(|(_, value)| value.as_str()),
                    Some(actual_value.as_str()),
                    "missing preserved env var {actual_key}"
                );
            }
        }
        assert!(present > 0, "test expected at least one Windows runtime env var");
    }

    #[test]
    #[cfg(windows)]
    fn platform_base_env_does_not_invent_unix_path_on_windows() {
        let env = platform_base_env();
        assert_ne!(
            env.iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
                .map(|(_, value)| value.as_str()),
            Some("/usr/local/bin:/usr/bin:/bin")
        );
    }

    #[test]
    fn default_config_validates() {
        let config = AgentConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn zero_max_iterations_invalid() {
        let config = AgentConfig { max_iterations: Some(0), ..AgentConfig::default() };
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
