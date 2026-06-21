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

        // ── Auto-allow safe shell commands ──────────────────────────
        if (tool_lower == "bash" || tool_lower == "shell" || tool_lower == "powershell")
            && let Some(cmd) = request.arguments.get("command").and_then(|v| v.as_str())
            && is_auto_allowed(&tool_lower, cmd)
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
fn is_auto_allowed(tool: &str, cmd: &str) -> bool {
    if tool.eq_ignore_ascii_case("powershell") {
        matches!(
            telos_agent::powershell_security::analyze(cmd),
            telos_agent::powershell_security::CommandSafety::Safe
        )
    } else {
        telos_agent::bash_security::analyze(cmd).is_safe()
    }
}

#[cfg(test)]
mod tests {
    use super::{TuiApprovalHandler, is_auto_allowed};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use telos_agent::{ApprovalDecision, ApprovalHandler, ApprovalRequest, Message};
    use tokio::sync::mpsc;

    #[test]
    fn auto_allow_dispatches_by_shell_tool() {
        assert!(is_auto_allowed("bash", "git status"));
        assert!(!is_auto_allowed("bash", "git status; rm -rf /"));
        assert!(!is_auto_allowed("bash", "python -c 'print(1)'"));
        assert!(is_auto_allowed("powershell", "Get-Process pwsh"));
        assert!(!is_auto_allowed("powershell", "Remove-Item -Recurse -Force ./target"));
    }

    fn approval_request(tool_name: &str, command: &str) -> ApprovalRequest {
        ApprovalRequest {
            tool_name: tool_name.into(),
            invocation_names: vec![tool_name.into()],
            arguments: serde_json::json!({ "command": command }),
            cwd: PathBuf::from("."),
            messages: Arc::new(vec![Message::user("hi")]),
            reason: "needs review".into(),
        }
    }

    #[tokio::test]
    async fn powershell_safe_command_auto_allows_without_queueing() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let handler = TuiApprovalHandler::new(tx, Arc::new(AtomicBool::new(false)));

        let decision = handler.ask(approval_request("PowerShell", "Get-Process pwsh")).await;

        assert_eq!(decision, ApprovalDecision::Allow);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn powershell_dangerous_command_queues_for_review() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let handler = TuiApprovalHandler::new(tx, Arc::new(AtomicBool::new(false)));
        let task = tokio::spawn(async move {
            handler
                .ask(approval_request("PowerShell", "Remove-Item -Recurse -Force ./target"))
                .await
        });

        let mut pending = rx.recv().await.expect("approval should be queued");
        let respond = pending.respond.take().expect("approval sender should exist");
        respond.send(ApprovalDecision::Deny { reason: "test".into() }).unwrap();

        assert_eq!(task.await.unwrap(), ApprovalDecision::Deny { reason: "test".into() });
    }
}
