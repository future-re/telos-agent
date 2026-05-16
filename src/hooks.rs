use async_trait::async_trait;
use std::sync::Arc;

use crate::error::AgentError;
use crate::message::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPhase {
    PostSampling,
    Stop,
}

#[derive(Debug, Clone)]
pub struct HookContext {
    pub session_id: String,
    pub turn_id: u64,
    pub message_count: usize,
}

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

    pub fn register<H>(&mut self, hook: H)
    where
        H: Hook + 'static,
    {
        self.hooks.push(Arc::new(hook));
    }

    pub fn hooks_for_phase(&self, phase: HookPhase) -> Vec<Arc<dyn Hook>> {
        self.hooks
            .iter()
            .filter(|hook| hook.phase() == phase)
            .cloned()
            .collect()
    }
}
