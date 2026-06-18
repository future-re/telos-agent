use telos_agent::{ApprovalDecision, ApprovalRequest};

#[derive(Debug)]
pub struct PendingApproval {
    pub request: ApprovalRequest,
    pub respond: tokio::sync::oneshot::Sender<ApprovalDecision>,
}
