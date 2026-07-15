//! Asynchronous human-in-the-loop approval for tool calls.
//!
//! When a tool or the permission engine returns [`PermissionDecision::Ask`](crate::PermissionDecision::Ask),
//! the runtime can suspend the turn and ask an [`ApprovalHandler`] to decide
//! whether to allow, deny, or modify the call.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::message::Message;

/// A request presented to an approval handler.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    /// ID of the tool call awaiting approval.
    pub tool_call_id: String,
    /// Canonical tool name.
    pub tool_name: String,
    /// Arguments the model supplied.
    pub arguments: Value,
    /// Working directory the tool will run in.
    pub cwd: PathBuf,
    /// Snapshot of the conversation up to (but not including) this tool call.
    pub messages: Arc<Vec<Message>>,
    /// Human-readable reason why approval is required.
    pub reason: String,
}

/// Decision returned by an approval handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Allow the tool call to proceed with the original arguments.
    Allow,
    /// Deny the tool call; the model receives an error result.
    Deny { reason: String },
    /// Allow the call but replace the arguments with modified ones.
    Modify { arguments: Value },
}

/// Handler invoked when a tool call requires explicit human approval.
#[async_trait]
pub trait ApprovalHandler: Send + Sync + std::fmt::Debug {
    /// Present the request to the user/host and return their decision.
    async fn ask(&self, request: ApprovalRequest) -> ApprovalDecision;
}

/// Built-in handler that always denies approval requests.
///
/// Useful as a safe default when no interactive handler is configured.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutoDenyHandler;

#[async_trait]
impl ApprovalHandler for AutoDenyHandler {
    async fn ask(&self, _request: ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Deny { reason: "no approval handler configured".into() }
    }
}

/// Test helper that always returns a fixed decision.
#[derive(Debug, Clone)]
pub struct FixedDecisionHandler {
    pub decision: ApprovalDecision,
}

#[async_trait]
impl ApprovalHandler for FixedDecisionHandler {
    async fn ask(&self, _request: ApprovalRequest) -> ApprovalDecision {
        self.decision.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn auto_deny_handler_always_denies() {
        let handler = AutoDenyHandler;
        let decision = handler
            .ask(ApprovalRequest {
                tool_call_id: "call-1".into(),
                tool_name: "Bash".into(),
                arguments: json!({"command": "rm -rf /"}),
                cwd: PathBuf::from("/tmp"),
                messages: Arc::new(vec![]),
                reason: "destructive command".into(),
            })
            .await;
        assert!(matches!(decision, ApprovalDecision::Deny { .. }));
    }

    #[tokio::test]
    async fn fixed_handler_returns_configured_decision() {
        let handler = FixedDecisionHandler { decision: ApprovalDecision::Allow };
        let decision = handler
            .ask(ApprovalRequest {
                tool_call_id: "call-1".into(),
                tool_name: "Read".into(),
                arguments: json!({"file_path": "/etc/passwd"}),
                cwd: PathBuf::from("/tmp"),
                messages: Arc::new(vec![]),
                reason: "test".into(),
            })
            .await;
        assert_eq!(decision, ApprovalDecision::Allow);
    }
}
