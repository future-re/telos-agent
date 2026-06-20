use serde::{Deserialize, Serialize};
use tauri::{Emitter, State, Window};
use tokio::sync::Mutex;

use crate::agent_host::{AgentHost, ChatSettings};

#[derive(Default)]
struct AppState {
    host: Mutex<Option<AgentHost>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptRequest {
    prompt: String,
    settings: ChatSettings,
    reset: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptResult {
    final_text: String,
}

#[tauri::command]
fn default_settings() -> ChatSettings {
    ChatSettings::default()
}

#[tauri::command]
async fn reset_session(state: State<'_, AppState>) -> Result<(), String> {
    let mut host = state.host.lock().await;
    *host = None;
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

    let final_text = host
        .run_prompt(prompt, |event| {
            let _ = window.emit("telos://event", event);
        })
        .await?;

    Ok(PromptResult { final_text })
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![default_settings, reset_session, send_prompt])
        .run(tauri::generate_context!())
        .expect("failed to run telos desktop");
}
