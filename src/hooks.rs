//! Hook system for intercepting assistant messages during a turn.
//!
//! Hooks let callers inject behaviour at well-defined points in the turn loop
//! without subclassing the session. A hook receives the current assistant
//! message and may optionally emit a *follow-up* message that gets appended to
//! the conversation (for example, to inject context or tool-use prompts).
//!
//! Hooks run at two phases:
//! - [`HookPhase::PostSampling`] — immediately after the model responds, on every iteration
//! - [`HookPhase::Stop`] — when the turn would end (no more tool calls)

use async_trait::async_trait;
use std::sync::Arc;

use crate::error::AgentError;
use crate::message::Message;

/// Lifecycle point at which a hook fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPhase {
    /// Right after the model returns a message — runs every iteration.
    PostSampling,
    /// When the turn is about to terminate naturally (no tool calls pending).
    Stop,
}

/// Metadata passed to a hook describing the session it's running inside.
#[derive(Debug, Clone)]
pub struct HookContext {
    pub session_id: String,
    pub turn_id: u64,
    /// Number of messages in the conversation when the hook fires.
    pub message_count: usize,
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

/// Collection of [`Hook`]s queried by phase during the turn loop.
///
/// `Clone` is cheap — hooks are stored behind `Arc`.
#[derive(Default, Clone)]
pub struct HookRegistry {
    hooks: Vec<Arc<dyn Hook>>,
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

    /// Register a hook. Hooks fire in registration order within each phase.
    pub fn register<H>(&mut self, hook: H)
    where
        H: Hook + 'static,
    {
        self.hooks.push(Arc::new(hook));
    }

    /// Snapshot of hooks for the given phase, preserving registration order.
    pub fn hooks_for_phase(&self, phase: HookPhase) -> Vec<Arc<dyn Hook>> {
        self.hooks.iter().filter(|hook| hook.phase() == phase).cloned().collect()
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
            self.phase
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
        assert!(r.hooks_for_phase(HookPhase::PostSampling).is_empty());
        assert!(r.hooks_for_phase(HookPhase::Stop).is_empty());
    }

    #[test]
    fn registry_filters_hooks_by_phase() {
        let mut r = HookRegistry::new();
        r.register(TestHook { name: "post", phase: HookPhase::PostSampling });
        r.register(TestHook { name: "stop", phase: HookPhase::Stop });
        assert_eq!(r.hooks_for_phase(HookPhase::PostSampling).len(), 1);
        assert_eq!(r.hooks_for_phase(HookPhase::Stop).len(), 1);
    }

    #[test]
    fn registry_preserves_registration_order() {
        let mut r = HookRegistry::new();
        r.register(TestHook { name: "a", phase: HookPhase::Stop });
        r.register(TestHook { name: "b", phase: HookPhase::Stop });
        let hooks = r.hooks_for_phase(HookPhase::Stop);
        assert_eq!(hooks[0].name(), "a");
        assert_eq!(hooks[1].name(), "b");
    }

    #[test]
    fn empty_registry_returns_empty_vec_for_any_phase() {
        let r = HookRegistry::new();
        assert!(r.hooks_for_phase(HookPhase::PostSampling).is_empty());
        assert!(r.hooks_for_phase(HookPhase::Stop).is_empty());
    }
}
