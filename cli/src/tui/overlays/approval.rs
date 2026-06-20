use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use telos_agent::{ApprovalDecision, ApprovalHandler, ApprovalRequest};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[derive(Debug)]
pub struct PendingApproval {
    pub request: ApprovalRequest,
    pub respond: Option<oneshot::Sender<ApprovalDecision>>,
}

pub struct TuiApprovalHandler {
    tx: mpsc::UnboundedSender<PendingApproval>,
    auto_mode: Arc<AtomicBool>,
}

impl TuiApprovalHandler {
    pub fn new(tx: mpsc::UnboundedSender<PendingApproval>, auto_mode: Arc<AtomicBool>) -> Self {
        Self { tx, auto_mode }
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
        // ── Auto mode: approve everything ─────────────────────────
        if self.auto_mode.load(Ordering::Relaxed) {
            return ApprovalDecision::Allow;
        }

        let tool_lower = request.tool_name.to_lowercase();

        // ── Auto-allow safe Bash commands ───────────────────────────
        if (tool_lower == "bash" || tool_lower == "shell")
            && let Some(cmd) = request.arguments.get("command").and_then(|v| v.as_str())
            && is_auto_allowed(cmd)
        {
            return ApprovalDecision::Allow;
        }

        // ── Auto-allow safe file operations ─────────────────────────
        if matches!(
            tool_lower.as_str(),
            "read" | "glob" | "grep" | "webfetch" | "websearch" | "task" | "askuserquestion"
        ) {
            return ApprovalDecision::Allow;
        }

        let (tx, rx) = oneshot::channel();
        let pending = PendingApproval { request, respond: Some(tx) };
        if self.tx.send(pending).is_err() {
            return ApprovalDecision::Deny { reason: "TUI approval channel closed".into() };
        }
        rx.await.unwrap_or(ApprovalDecision::Deny { reason: "no response from user".into() })
    }
}

/// Decide whether a shell command should run without explicit approval.
fn is_auto_allowed(cmd: &str) -> bool {
    telos_agent::bash_security::analyze(cmd).is_safe()
}

#[cfg(test)]
mod tests {
    use super::is_auto_allowed;

    #[test]
    fn auto_allow_uses_bash_security_analysis_only() {
        assert!(is_auto_allowed("git status"));
        assert!(!is_auto_allowed("git status; rm -rf /"));
        assert!(!is_auto_allowed("python -c 'print(1)'"));
    }
}
