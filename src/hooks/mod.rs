//! Hook system for intercepting assistant messages during a turn.
//!
//! Hooks let callers inject behaviour at well-defined points in the turn loop
//! without subclassing the session. A hook receives the current assistant
//! message and may optionally emit a *follow-up* message that gets appended to
//! the conversation (for example, to inject context or tool-use prompts).
//!
//! Hooks run at these phases:
//! - [`HookPhase::PostSampling`] — immediately after the model responds, on every iteration
//! - [`HookPhase::Stop`] — when the turn would end (no more tool calls)
//! - [`HookPhase::PreToolUse`] — before a specific tool runs
//! - [`HookPhase::PostToolUse`] — after a specific tool succeeds
//! - [`HookPhase::PostToolUseFailure`] — after a specific tool fails
//! - [`HookPhase::SessionStart`] — when a new session is created
//! - [`HookPhase::UserPromptSubmit`] — when the user submits a prompt

pub mod http;
pub mod prompt;

use async_trait::async_trait;
use std::sync::Arc;

use crate::error::AgentError;
use crate::message::Message;

/// Lifecycle point at which a hook fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookPhase {
    /// Right after the model returns a message — runs every iteration.
    PostSampling,
    /// When the turn is about to terminate naturally (no tool calls pending).
    Stop,
    /// Before a specific tool is invoked.
    PreToolUse { tool_name: String },
    /// After a specific tool succeeds.
    PostToolUse { tool_name: String },
    /// After a specific tool fails.
    PostToolUseFailure { tool_name: String },
    /// When a new session starts.
    SessionStart,
    /// When the user submits a prompt.
    UserPromptSubmit,
}

impl HookPhase {
    /// Human-readable phase name for logging and event reporting.
    pub fn name(&self) -> &'static str {
        match self {
            HookPhase::PostSampling => "post_sampling",
            HookPhase::Stop => "stop",
            HookPhase::PreToolUse { .. } => "pre_tool_use",
            HookPhase::PostToolUse { .. } => "post_tool_use",
            HookPhase::PostToolUseFailure { .. } => "post_tool_use_failure",
            HookPhase::SessionStart => "session_start",
            HookPhase::UserPromptSubmit => "user_prompt_submit",
        }
    }
}

/// Metadata passed to a hook describing the session it's running inside.
#[derive(Debug, Clone)]
pub struct HookContext {
    pub session_id: String,
    pub turn_id: u64,
    /// Number of messages in the conversation when the hook fires.
    pub message_count: usize,
}

/// Condition that further refines when a hook fires within a given phase.
///
/// # Matching rules
///
/// | Condition `tool_name` | Phase `tool_name` | Matches? |
/// |---|---:|---:|
/// | `None` | any | yes |
/// | `Some("Bash")` | `"Bash"` | yes |
/// | `Some("Bash")` | `"Grep"` | no |
/// | `Some("Bash(git *)")` | `"Bash(git status)"` | yes |
/// | `Some("Bash(git *)")` | `"Bash"` | no |
/// | `Some("*")` | any | yes |
#[derive(Debug, Clone)]
pub struct HookCondition {
    /// Optional tool name pattern. Supports exact match, glob (`*`), and `"*"` wildcard.
    /// Only meaningful for phases that carry a `tool_name`
    /// (e.g. `PreToolUse`, `PostToolUse`, `PostToolUseFailure`).
    pub tool_name: Option<String>,
}

impl HookCondition {
    /// Check whether this condition matches the given phase.
    ///
    /// When `tool_name` is `None`, every phase matches. When set, only
    /// phases that carry a `tool_name` and whose `tool_name` matches the
    /// pattern (or the pattern is `"*"`) pass.
    pub fn matches(&self, phase: &HookPhase) -> bool {
        let Some(pattern) = &self.tool_name else {
            return true;
        };
        match phase {
            HookPhase::PreToolUse { tool_name }
            | HookPhase::PostToolUse { tool_name }
            | HookPhase::PostToolUseFailure { tool_name } => matches_tool_name(pattern, tool_name),
            _ => false,
        }
    }
}

/// Check whether `tool_name` matches the glob `pattern`.
fn matches_tool_name(pattern: &str, tool_name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    glob::Pattern::new(pattern).is_ok_and(|p| p.matches(tool_name))
}

/// A hook wrapped with execution metadata.
///
/// Stores the hook itself together with its phase, optional condition,
/// and lifecycle flags.
#[derive(Clone)]
pub struct HookEntry {
    pub hook: Arc<dyn Hook>,
    pub phase: HookPhase,
    pub condition: Option<HookCondition>,
    /// If `true`, the hook is removed after its first execution.
    pub once: bool,
    /// If `true`, the hook runs asynchronously (fire-and-forget).
    pub async_exec: bool,
}

impl std::fmt::Debug for HookEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookEntry")
            .field("hook", &self.hook.name())
            .field("phase", &self.phase)
            .field("condition", &self.condition)
            .field("once", &self.once)
            .field("async_exec", &self.async_exec)
            .finish()
    }
}

/// Trait for a hook that observes (and optionally augments) the assistant message.
///
/// Returning `Some(message)` from [`run`](Hook::run) appends that message to the
/// conversation in addition to the assistant's own message; returning `None`
/// leaves the conversation unchanged.
#[async_trait]
pub trait Hook: Send + Sync {
    fn name(&self) -> &str;
    fn phase(&self) -> HookPhase;

    async fn run(
        &self,
        context: &HookContext,
        message: &Message,
    ) -> Result<Option<Message>, AgentError>;
}

/// Collection of [`HookEntry`]s queried by phase during the turn loop.
///
/// `Clone` is cheap — hooks are stored behind `Arc`.
#[derive(Default, Clone)]
pub struct HookRegistry {
    hooks: Vec<HookEntry>,
}

impl std::fmt::Debug for HookRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookRegistry").field("hook_count", &self.hooks.len()).finish()
    }
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a hook entry. Hooks fire in registration order within each phase.
    pub fn register_entry(&mut self, entry: HookEntry) {
        self.hooks.push(entry);
    }

    /// Register a hook directly (backward-compatible).
    ///
    /// This wraps the hook in a [`HookEntry`] with default metadata (no condition,
    /// not once, not async). Use [`register_entry`](Self::register_entry) for
    /// full control.
    pub fn register<H>(&mut self, hook: H)
    where
        H: Hook + 'static,
    {
        let phase = hook.phase();
        self.hooks.push(HookEntry {
            hook: Arc::new(hook),
            phase,
            condition: None,
            once: false,
            async_exec: false,
        });
    }

    /// Snapshot of hooks for the given phase, filtered by conditions,
    /// preserving registration order.
    pub fn hooks_for_phase(&self, phase: &HookPhase) -> Vec<Arc<dyn Hook>> {
        self.hooks
            .iter()
            .filter(|entry| {
                // Match on phase discriminant only (ignore tool_name in the variant).
                if std::mem::discriminant(&entry.phase) != std::mem::discriminant(phase) {
                    return false;
                }
                if let Some(condition) = &entry.condition { condition.matches(phase) } else { true }
            })
            .map(|entry| entry.hook.clone())
            .collect()
    }

    /// Remove all hooks that have `once: true`.
    ///
    /// Call this after each phase execution to clean up one-shot hooks.
    pub fn remove_once_hooks(&mut self) {
        self.hooks.retain(|entry| !entry.once);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct TestHook {
        name: &'static str,
        phase: HookPhase,
    }

    #[async_trait]
    impl Hook for TestHook {
        fn name(&self) -> &str {
            self.name
        }
        fn phase(&self) -> HookPhase {
            self.phase.clone()
        }
        async fn run(
            &self,
            _ctx: &HookContext,
            _msg: &Message,
        ) -> Result<Option<Message>, AgentError> {
            Ok(None)
        }
    }

    #[test]
    fn registry_is_empty_by_default() {
        let r = HookRegistry::new();
        assert!(r.hooks_for_phase(&HookPhase::PostSampling).is_empty());
        assert!(r.hooks_for_phase(&HookPhase::Stop).is_empty());
    }

    #[test]
    fn registry_filters_hooks_by_phase() {
        let mut r = HookRegistry::new();
        r.register(TestHook { name: "post", phase: HookPhase::PostSampling });
        r.register(TestHook { name: "stop", phase: HookPhase::Stop });
        assert_eq!(r.hooks_for_phase(&HookPhase::PostSampling).len(), 1);
        assert_eq!(r.hooks_for_phase(&HookPhase::Stop).len(), 1);
    }

    #[test]
    fn registry_preserves_registration_order() {
        let mut r = HookRegistry::new();
        r.register(TestHook { name: "a", phase: HookPhase::Stop });
        r.register(TestHook { name: "b", phase: HookPhase::Stop });
        let hooks = r.hooks_for_phase(&HookPhase::Stop);
        assert_eq!(hooks[0].name(), "a");
        assert_eq!(hooks[1].name(), "b");
    }

    #[test]
    fn empty_registry_returns_empty_vec_for_any_phase() {
        let r = HookRegistry::new();
        assert!(r.hooks_for_phase(&HookPhase::PostSampling).is_empty());
        assert!(r.hooks_for_phase(&HookPhase::Stop).is_empty());
    }

    #[test]
    fn hook_condition_exact_match() {
        let cond = HookCondition { tool_name: Some("Bash".into()) };
        assert!(cond.matches(&HookPhase::PreToolUse { tool_name: "Bash".into() }));
        assert!(!cond.matches(&HookPhase::PreToolUse { tool_name: "Grep".into() }));
        assert!(!cond.matches(&HookPhase::PostSampling));
    }

    #[test]
    fn hook_condition_glob_match() {
        let cond = HookCondition { tool_name: Some("Bash(git *)".into()) };
        assert!(cond.matches(&HookPhase::PreToolUse { tool_name: "Bash(git status)".into() }));
        assert!(!cond.matches(&HookPhase::PreToolUse { tool_name: "Bash".into() }));
        assert!(!cond.matches(&HookPhase::PostSampling));
    }

    #[test]
    fn hook_condition_wildcard_matches_any() {
        let cond = HookCondition { tool_name: Some("*".into()) };
        assert!(cond.matches(&HookPhase::PreToolUse { tool_name: "Bash".into() }));
        assert!(cond.matches(&HookPhase::PreToolUse { tool_name: "Anything".into() }));
        assert!(!cond.matches(&HookPhase::PostSampling));
    }

    #[test]
    fn hook_condition_none_matches_all() {
        let cond = HookCondition { tool_name: None };
        assert!(cond.matches(&HookPhase::PreToolUse { tool_name: "Bash".into() }));
        assert!(cond.matches(&HookPhase::PostSampling));
    }

    #[test]
    fn hooks_for_phase_filters_by_condition() {
        let mut r = HookRegistry::new();

        // Hook that only fires for PreToolUse with tool_name "Bash"
        r.register_entry(HookEntry {
            hook: Arc::new(TestHook {
                name: "bash-only",
                phase: HookPhase::PreToolUse { tool_name: "".into() },
            }),
            phase: HookPhase::PreToolUse { tool_name: "".into() },
            condition: Some(HookCondition { tool_name: Some("Bash".into()) }),
            once: false,
            async_exec: false,
        });

        // Hook that fires for any PreToolUse
        r.register_entry(HookEntry {
            hook: Arc::new(TestHook {
                name: "any-tool",
                phase: HookPhase::PreToolUse { tool_name: "".into() },
            }),
            phase: HookPhase::PreToolUse { tool_name: "".into() },
            condition: None,
            once: false,
            async_exec: false,
        });

        let bash_hooks = r.hooks_for_phase(&HookPhase::PreToolUse { tool_name: "Bash".into() });
        assert_eq!(bash_hooks.len(), 2, "both hooks should match for Bash");

        let grep_hooks = r.hooks_for_phase(&HookPhase::PreToolUse { tool_name: "Grep".into() });
        assert_eq!(grep_hooks.len(), 1, "only the unconditional hook should match for Grep");
        assert_eq!(grep_hooks[0].name(), "any-tool");
    }

    #[test]
    fn remove_once_hooks_cleans_up() {
        let mut r = HookRegistry::new();
        r.register_entry(HookEntry {
            hook: Arc::new(TestHook { name: "once", phase: HookPhase::PostSampling }),
            phase: HookPhase::PostSampling,
            condition: None,
            once: true,
            async_exec: false,
        });
        r.register_entry(HookEntry {
            hook: Arc::new(TestHook { name: "persistent", phase: HookPhase::PostSampling }),
            phase: HookPhase::PostSampling,
            condition: None,
            once: false,
            async_exec: false,
        });

        assert_eq!(r.hooks_for_phase(&HookPhase::PostSampling).len(), 2);
        r.remove_once_hooks();
        let remaining = r.hooks_for_phase(&HookPhase::PostSampling);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name(), "persistent");
    }

    #[test]
    fn hook_phase_name_returns_static_str() {
        assert_eq!(HookPhase::PostSampling.name(), "post_sampling");
        assert_eq!(HookPhase::Stop.name(), "stop");
        assert_eq!(HookPhase::PreToolUse { tool_name: "Bash".into() }.name(), "pre_tool_use");
        assert_eq!(HookPhase::SessionStart.name(), "session_start");
        assert_eq!(HookPhase::UserPromptSubmit.name(), "user_prompt_submit");
    }
}
