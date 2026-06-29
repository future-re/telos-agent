use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, Manager, State, Window};
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;

use crate::agent_host::{
    AgentHost, DesktopSettingsOverrides, MemoryOverview, ResolvedDesktopSettings, SessionSummary,
    delete_session_files, list_sessions as list_sessions_impl, load_session_messages,
    memory_overview, resolve_desktop_settings, save_deepseek_api_key,
};

type HostMap = Mutex<HashMap<String, HostEntry>>;
type CancelMap = Mutex<HashMap<String, CancellationToken>>;
type PendingApprovalMap = Arc<Mutex<HashMap<String, PendingApprovalEntry>>>;

struct HostEntry {
    settings: DesktopSettingsOverrides,
    host: Arc<Mutex<AgentHost>>,
}

struct PendingApprovalEntry {
    session_id: String,
    sender: oneshot::Sender<telos_agent::ApprovalDecision>,
}

#[derive(Default)]
struct AppState {
    hosts: HostMap,
    cancels: CancelMap,
    approvals: PendingApprovalMap,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptRequest {
    session_id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncedMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtractResult {
    text: String,
    messages: Vec<SyncedMessage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveDeepSeekKeyRequest {
    api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionRequest {
    session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveApprovalRequest {
    session_id: String,
    approval_id: String,
    decision: String,
    arguments: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadSessionRequest {
    session_id: String,
    cwd: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopApprovalEvent {
    kind: &'static str,
    session_id: String,
    approval_id: String,
    tool_call_id: String,
    tool_name: String,
    arguments: Value,
    cwd: String,
    reason: String,
}

#[derive(Clone)]
struct DesktopApprovalHandler {
    session_id: String,
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
        self.approvals.lock().await.insert(
            approval_id.clone(),
            PendingApprovalEntry { session_id: self.session_id.clone(), sender: tx },
        );

        let event = DesktopApprovalEvent {
            kind: "approval_required",
            session_id: self.session_id.clone(),
            approval_id: approval_id.clone(),
            tool_call_id: request.tool_call_id,
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
async fn list_sessions(
    request: Option<ResolveSettingsRequest>,
) -> Result<Vec<SessionSummary>, String> {
    list_sessions_impl(request.and_then(|r| r.cwd)).await
}

#[tauri::command]
async fn load_session(request: LoadSessionRequest) -> Result<Vec<serde_json::Value>, String> {
    load_session_messages(&request.session_id, request.cwd).await
}

#[tauri::command]
async fn extract_deepseek_text(window: Window) -> Result<ExtractResult, String> {
    let webview =
        window.get_webview("deepseek-panel").ok_or_else(|| "DeepSeek 面板未打开".to_string())?;
    let url = webview.url().map_err(|e| format!("无法读取 DeepSeek 面板地址：{}", e))?;
    if url.scheme() != "https" || url.host_str() != Some("chat.deepseek.com") {
        return Err("DeepSeek 面板地址不可信，已拒绝同步".to_string());
    }

    let (tx, rx) = oneshot::channel();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));

    let js = r#"(function() {
    try {
        var text = document.body.innerText.trim().substring(0, 50000);
        return {text: text, messages: []};
    } catch(e) {
        return {text: 'ERR: ' + e.message, messages: []};
    }
})();"#;

    webview
        .eval_with_callback(js, move |result| {
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(result);
            }
        })
        .map_err(|e| format!("无法注入提取脚本：{}", e))?;

    let json_text = tokio::time::timeout(std::time::Duration::from_secs(10), rx)
        .await
        .map_err(|_| "提取超时，请确保 DeepSeek 面板已加载完毕".to_string())?
        .unwrap_or_else(|_| "\"\"".to_string());

    let extract: ExtractResult = serde_json::from_str(&json_text).unwrap_or_else(|_| {
        ExtractResult { text: json_text.trim_matches('"').to_string(), messages: vec![] }
    });

    Ok(extract)
}

#[tauri::command]
async fn reset_all_sessions(state: State<'_, AppState>) -> Result<(), String> {
    let tokens = state.cancels.lock().await.drain().map(|(_, token)| token).collect::<Vec<_>>();
    for token in tokens {
        token.cancel();
    }
    deny_pending_approvals(&state.approvals, None, "session reset").await;
    state.hosts.lock().await.clear();
    Ok(())
}

#[tauri::command]
async fn reset_session(state: State<'_, AppState>, request: SessionRequest) -> Result<(), String> {
    if let Some(token) = state.cancels.lock().await.remove(&request.session_id) {
        token.cancel();
    }
    deny_pending_approvals(&state.approvals, Some(&request.session_id), "session reset").await;
    let cwd = state.hosts.lock().await.remove(&request.session_id).and_then(|e| e.settings.cwd);
    delete_session_files(&request.session_id, cwd).await.ok();
    Ok(())
}

#[tauri::command]
async fn cancel_current_task(
    state: State<'_, AppState>,
    request: SessionRequest,
) -> Result<(), String> {
    if let Some(token) = state.cancels.lock().await.remove(&request.session_id) {
        token.cancel();
    }
    deny_pending_approvals(&state.approvals, Some(&request.session_id), "task cancelled").await;
    Ok(())
}

#[tauri::command]
async fn resolve_approval(
    state: State<'_, AppState>,
    request: ResolveApprovalRequest,
) -> Result<(), String> {
    let entry = state
        .approvals
        .lock()
        .await
        .remove(&request.approval_id)
        .ok_or_else(|| "Approval request is no longer pending".to_string())?;

    if entry.session_id != request.session_id {
        return Err("Approval request belongs to a different session".to_string());
    }

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

    entry.sender.send(decision).map_err(|_| "Approval request already closed".to_string())
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

    let session_id = request.session_id.clone();
    if request.reset.unwrap_or(false) {
        if let Some(token) = state.cancels.lock().await.remove(&session_id) {
            token.cancel();
        }
        deny_pending_approvals(&state.approvals, Some(&session_id), "session reset").await;
    }
    let host = {
        let mut hosts = state.hosts.lock().await;
        if request.reset.unwrap_or(false) {
            hosts.remove(&session_id);
        }
        if let Some(existing) = hosts.get(&session_id)
            && existing.settings == request.settings
        {
            existing.host.clone()
        } else {
            let manual_approval_handler = Arc::new(DesktopApprovalHandler {
                session_id: session_id.clone(),
                window: window.clone(),
                approvals: state.approvals.clone(),
            });
            let settings = request.settings.clone();
            let host = Arc::new(Mutex::new(AgentHost::new(
                settings.clone(),
                Some(manual_approval_handler),
            )?));
            hosts.insert(session_id.clone(), HostEntry { settings, host: host.clone() });
            host
        }
    };

    let token = CancellationToken::new();
    state.cancels.lock().await.insert(session_id.clone(), token.clone());

    let final_text = tokio::select! {
        result = async {
            let mut host = host.lock().await;
            host.run_prompt(&session_id, prompt, |event| {
                let _ = window.emit("telos://event", event);
            }).await
        } => result?,
        _ = token.cancelled() => {
            deny_pending_approvals(&state.approvals, Some(&session_id), "task cancelled").await;
            let _ = window.emit("telos://event", serde_json::json!({
                "kind": "cancelled",
                "sessionId": session_id,
                "message": "已停止当前任务"
            }));
            String::new()
        }
    };

    state.cancels.lock().await.remove(&request.session_id);

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
            list_sessions,
            load_session,
            extract_deepseek_text,
            reset_all_sessions,
            reset_session,
            cancel_current_task,
            resolve_approval,
            send_prompt
        ])
        .run(tauri::generate_context!())
        .expect("failed to run telos desktop");
}

async fn deny_pending_approvals(
    approvals: &PendingApprovalMap,
    session_id: Option<&str>,
    reason: &str,
) {
    let mut pending = approvals.lock().await;
    let approval_ids = pending
        .iter()
        .filter(|(_, entry)| session_id.is_none_or(|id| id == entry.session_id))
        .map(|(approval_id, _)| approval_id.clone())
        .collect::<Vec<_>>();

    let senders = approval_ids
        .into_iter()
        .filter_map(|approval_id| pending.remove(&approval_id).map(|entry| entry.sender))
        .collect::<Vec<_>>();
    drop(pending);

    for sender in senders {
        let _ = sender.send(telos_agent::ApprovalDecision::Deny { reason: reason.into() });
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
