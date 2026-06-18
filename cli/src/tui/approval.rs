use async_trait::async_trait;
use telos_agent::{ApprovalDecision, ApprovalHandler, ApprovalRequest};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[derive(Debug)]
pub struct PendingApproval {
    pub request: ApprovalRequest,
    pub respond: oneshot::Sender<ApprovalDecision>,
}

pub struct TuiApprovalHandler {
    tx: mpsc::UnboundedSender<PendingApproval>,
}

impl TuiApprovalHandler {
    pub fn new(tx: mpsc::UnboundedSender<PendingApproval>) -> Self {
        Self { tx }
    }
}

impl std::fmt::Debug for TuiApprovalHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TuiApprovalHandler").finish_non_exhaustive()
    }
}

#[async_trait]
impl ApprovalHandler for TuiApprovalHandler {
    async fn ask(&self, request: ApprovalRequest) -> ApprovalDecision {
        let (tx, rx) = oneshot::channel();
        let pending = PendingApproval { request, respond: tx };
        if self.tx.send(pending).is_err() {
            return ApprovalDecision::Deny { reason: "TUI approval channel closed".into() };
        }
        rx.await.unwrap_or(ApprovalDecision::Deny { reason: "no response from user".into() })
    }
}
