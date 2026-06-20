use serde::{Deserialize, Serialize};
use tauri::{Emitter, State, Window};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::agent_host::{
    AgentHost, DesktopSettingsOverrides, MemoryOverview, ResolvedDesktopSettings, memory_overview,
    resolve_desktop_settings, save_deepseek_api_key,
};

#[derive(Default)]
struct AppState {
    host: Mutex<Option<AgentHost>>,
    cancel: Mutex<Option<CancellationToken>>,
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
    let mut host = state.host.lock().await;
    *host = None;
    Ok(())
}

#[tauri::command]
async fn cancel_current_task(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(token) = state.cancel.lock().await.take() {
        token.cancel();
    }
    Ok(())
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
        *host = Some(AgentHost::new(request.settings)?);
    }
    let host = host.as_mut().ok_or_else(|| "Agent host failed to initialize".to_string())?;

    let token = CancellationToken::new();
    *state.cancel.lock().await = Some(token.clone());

    let final_text = tokio::select! {
        result = host.run_prompt(prompt, |event| {
            let _ = window.emit("telos://event", event);
        }) => result?,
        _ = token.cancelled() => {
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
            send_prompt
        ])
        .run(tauri::generate_context!())
        .expect("failed to run telos desktop");
}
