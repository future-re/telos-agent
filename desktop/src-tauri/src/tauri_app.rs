use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, State, Window};
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;

use crate::agent_host::{
    AgentHost, DesktopSettingsOverrides, MemoryOverview, ResolvedDesktopSettings, memory_overview,
    resolve_desktop_settings, save_deepseek_api_key,
};

type PendingApprovalMap =
    Arc<Mutex<HashMap<String, oneshot::Sender<telos_agent::ApprovalDecision>>>>;

#[derive(Default)]
struct AppState {
    host: Mutex<Option<AgentHost>>,
    cancel: Mutex<Option<CancellationToken>>,
    approvals: PendingApprovalMap,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptRequest {
    prompt: String,
    settings: DesktopSettingsOverrides,
    reset: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptResult {
    final_text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveSettingsRequest {
    cwd: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveDeepSeekKeyRequest {
    api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveApprovalRequest {
    approval_id: String,
    decision: String,
    arguments: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopApprovalEvent {
    kind: &'static str,
    approval_id: String,
    tool_name: String,
    arguments: Value,
    cwd: String,
    reason: String,
}

#[derive(Clone)]
struct DesktopApprovalHandler {
    window: Window,
    approvals: PendingApprovalMap,
}

impl std::fmt::Debug for DesktopApprovalHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopApprovalHandler").finish_non_exhaustive()
    }
}

#[async_trait]
impl telos_agent::ApprovalHandler for DesktopApprovalHandler {
    async fn ask(&self, request: telos_agent::ApprovalRequest) -> telos_agent::ApprovalDecision {
        let approval_id = new_approval_id();
        let (tx, rx) = oneshot::channel();
        self.approvals.lock().await.insert(approval_id.clone(), tx);

        let event = DesktopApprovalEvent {
            kind: "approval_required",
            approval_id: approval_id.clone(),
            tool_name: request.tool_name,
            arguments: request.arguments,
            cwd: request.cwd.display().to_string(),
            reason: request.reason,
        };

        if self.window.emit("telos://event", event).is_err() {
            self.approvals.lock().await.remove(&approval_id);
            return telos_agent::ApprovalDecision::Deny {
                reason: "approval UI unavailable".into(),
            };
        }

        rx.await.unwrap_or(telos_agent::ApprovalDecision::Deny {
            reason: "approval response channel closed".into(),
        })
    }
}

#[tauri::command]
fn resolved_settings(
    request: Option<ResolveSettingsRequest>,
) -> Result<ResolvedDesktopSettings, String> {
    resolve_desktop_settings(&DesktopSettingsOverrides {
        cwd: request.and_then(|request| request.cwd),
        ..DesktopSettingsOverrides::default()
    })
}

#[tauri::command]
fn save_deepseek_key(request: SaveDeepSeekKeyRequest) -> Result<ResolvedDesktopSettings, String> {
    save_deepseek_api_key(&request.api_key)
}

#[tauri::command]
fn memory_summary(request: Option<ResolveSettingsRequest>) -> Result<MemoryOverview, String> {
    memory_overview(&DesktopSettingsOverrides {
        cwd: request.and_then(|request| request.cwd),
        ..DesktopSettingsOverrides::default()
    })
}

#[tauri::command]
async fn reset_session(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(token) = state.cancel.lock().await.take() {
        token.cancel();
    }
    deny_pending_approvals(&state.approvals, "session reset").await;
    let mut host = state.host.lock().await;
    *host = None;
    Ok(())
}

#[tauri::command]
async fn cancel_current_task(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(token) = state.cancel.lock().await.take() {
        token.cancel();
    }
    deny_pending_approvals(&state.approvals, "task cancelled").await;
    Ok(())
}

#[tauri::command]
async fn resolve_approval(
    state: State<'_, AppState>,
    request: ResolveApprovalRequest,
) -> Result<(), String> {
    let tx = state
        .approvals
        .lock()
        .await
        .remove(&request.approval_id)
        .ok_or_else(|| "Approval request is no longer pending".to_string())?;

    let decision = match request.decision.as_str() {
        "allow" => telos_agent::ApprovalDecision::Allow,
        "deny" => telos_agent::ApprovalDecision::Deny { reason: "denied by user".into() },
        "modify" => telos_agent::ApprovalDecision::Modify {
            arguments: request
                .arguments
                .ok_or_else(|| "Modified approval requires arguments".to_string())?,
        },
        other => return Err(format!("Unknown approval decision: {other}")),
    };

    tx.send(decision).map_err(|_| "Approval request already closed".to_string())
}

#[tauri::command]
async fn send_prompt(
    window: Window,
    state: State<'_, AppState>,
    request: PromptRequest,
) -> Result<PromptResult, String> {
    let prompt = request.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("Prompt cannot be empty".into());
    }

    let mut host = state.host.lock().await;
    if request.reset.unwrap_or(false) || host.is_none() {
        let manual_approval_handler = Arc::new(DesktopApprovalHandler {
            window: window.clone(),
            approvals: state.approvals.clone(),
        });
        *host = Some(AgentHost::new(request.settings, Some(manual_approval_handler))?);
    }
    let host = host.as_mut().ok_or_else(|| "Agent host failed to initialize".to_string())?;

    let token = CancellationToken::new();
    *state.cancel.lock().await = Some(token.clone());

    let final_text = tokio::select! {
        result = host.run_prompt(prompt, |event| {
            let _ = window.emit("telos://event", event);
        }) => result?,
        _ = token.cancelled() => {
            deny_pending_approvals(&state.approvals, "task cancelled").await;
            let _ = window.emit("telos://event", serde_json::json!({
                "kind": "cancelled",
                "message": "已停止当前任务"
            }));
            String::new()
        }
    };

    *state.cancel.lock().await = None;

    Ok(PromptResult { final_text })
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            resolved_settings,
            save_deepseek_key,
            memory_summary,
            reset_session,
            cancel_current_task,
            resolve_approval,
            send_prompt
        ])
        .run(tauri::generate_context!())
        .expect("failed to run telos desktop");
}

async fn deny_pending_approvals(approvals: &PendingApprovalMap, reason: &str) {
    let pending = approvals.lock().await.drain().map(|(_, tx)| tx).collect::<Vec<_>>();
    for tx in pending {
        let _ = tx.send(telos_agent::ApprovalDecision::Deny { reason: reason.into() });
    }
}

fn new_approval_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_APPROVAL_ID: AtomicU64 = AtomicU64::new(1);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = NEXT_APPROVAL_ID.fetch_add(1, Ordering::Relaxed);
    format!("approval-{timestamp}-{sequence}")
}
