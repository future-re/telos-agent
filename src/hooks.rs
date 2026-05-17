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
        f.debug_struct("HookRegistry")
            .field("hook_count", &self.hooks.len())
            .finish()
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
        self.hooks
            .iter()
            .filter(|hook| hook.phase() == phase)
            .cloned()
            .collect()
    }
}
