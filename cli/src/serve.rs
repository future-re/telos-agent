//! `telos serve` — JSON-line daemon mode.
//!
//! Reads one JSON command per line from stdin and emits JSON events per line
//! on stdout. Designed as the backend for a Python Textual TUI.
//!
//! # Commands (stdin, one JSON object per line)
//!
//! | field | type | description |
//! |---|---|---|
//! | `cmd` | string | `"run"`, `"new_session"`, `"_approve"`, `"quit"` |
//! | `prompt` | string? | user input for `"run"` |
//! | `tool_call_id` | string? | tool call ID for `"_approve"` |
//! | `decision` | string? | `"allow"` or `"deny"` for `"_approve"` |
//!
//! # Events (stdout, one JSON object per line)
//!
//! Turn events are serialized with an added `"type"` field. Extra control
//! events: `{"type":"_done"}`, `{"type":"_error","message":"…"}`,
//! `{"type":"_approval_required","tool_call_id":"…","name":"…",...}`.
//!
//! # Approval flow
//!
//! When a tool requires approval, the daemon emits `_approval_required` and
//! blocks the turn until it receives `{"cmd":"_approve","decision":"allow"}`.

use std::io::Write;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use telos_agent::{AgentSession, ApprovalDecision, ApprovalHandler, ApprovalRequest};
use tokio::io::AsyncBufReadExt;
use tokio::sync::oneshot;

use crate::cli::SharedOptions;
use crate::config::FileConfig;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ServeCommand {
    cmd: String,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    decision: Option<String>,
}

/// Approval handler that suspends the turn and waits for the Python
/// frontend to send back `{"cmd":"_approve","decision":"allow"}` via stdin.
#[derive(Debug, Clone)]
struct ServeApprovalHandler {
    pending: Arc<Mutex<Option<oneshot::Sender<ApprovalDecision>>>>,
}

#[async_trait]
impl ApprovalHandler for ServeApprovalHandler {
    async fn ask(&self, request: ApprovalRequest) -> ApprovalDecision {
        let (tx, rx) = oneshot::channel();
        *self.pending.lock().unwrap() = Some(tx);

        let event = serde_json::json!({
            "type": "_approval_required",
            "tool_call_id": request.tool_call_id,
            "name": request.tool_name,
            "arguments": request.arguments,
            "reason": request.reason,
        });
        println!("{}", serde_json::to_string(&event).unwrap_or_default());
        let _ = std::io::stdout().flush();

        rx.await.unwrap_or(ApprovalDecision::Deny {
            reason: "approval timeout / channel closed".into(),
        })
    }
}

/// Run the JSON-line daemon.
pub async fn run_serve(options: &SharedOptions, file_config: &FileConfig) -> Result<()> {
    let mut runtime = crate::runtime::prepare_runtime(options, file_config, None)?;
    let provider: Arc<dyn telos_agent::ModelProvider> =
        crate::build_erased_provider(options, file_config)?;
    crate::runtime::register_cli_subagent_tool(
        &mut runtime.shared.tools,
        &runtime.shared.agent_config,
        provider.clone(),
    )?;
    crate::runtime::rebuild_prompt_assembly(&mut runtime);

    // Shared state for approval handshake.
    let pending: Arc<Mutex<Option<oneshot::Sender<ApprovalDecision>>>> = Arc::new(Mutex::new(None));
    runtime.shared.agent_config.approval_handler =
        Some(Arc::new(ServeApprovalHandler { pending: pending.clone() }));

    let agent_runtime = telos_agent::AgentRuntime::new(
        runtime.shared.agent_config.clone(),
        provider,
        runtime.shared.tools.clone(),
    )?;
    let mut session = agent_runtime.create_session().await?;

    let stdin = tokio::io::stdin();
    let reader = tokio::io::BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        let command: ServeCommand = match serde_json::from_str(&trimmed) {
            Ok(cmd) => cmd,
            Err(e) => {
                emit_error(&format!("invalid command: {e}"));
                continue;
            }
        };

        match command.cmd.as_str() {
            "run" => {
                let prompt = match command.prompt {
                    Some(p) => p,
                    None => {
                        emit_error("'run' requires a 'prompt' field");
                        continue;
                    }
                };
                run_turn(&agent_runtime, &session, prompt).await;
            }
            "new_session" => {
                session = agent_runtime.create_session().await?;
                emit_event("_session_new");
            }
            "quit" | "exit" => break,
            "_approve" => {
                let decision = match command.decision.as_deref().unwrap_or("deny") {
                    "allow" | "yes" | "y" => ApprovalDecision::Allow,
                    _ => ApprovalDecision::Deny { reason: "denied by user".into() },
                };
                if let Some(tx) = pending.lock().unwrap().take() {
                    let _ = tx.send(decision);
                }
                // If no pending approval, it's a no-op (stale response).
            }
            other => {
                emit_error(&format!("unknown command: {other}"));
            }
        }
    }

    Ok(())
}

async fn run_turn(runtime: &telos_agent::AgentRuntime, session: &AgentSession, prompt: String) {
    let mut stream = match runtime.start_turn(session, prompt) {
        Ok(stream) => stream,
        Err(error) => {
            emit_error(&error.to_string());
            emit_done();
            return;
        }
    };
    while let Some(event) = stream.next().await {
        let mut value = serde_json::to_value(&event).unwrap_or_default();
        let type_name = event_variant_name(&event);
        if let Some(obj) = value.as_object_mut() {
            obj.insert("type".into(), serde_json::Value::String(type_name.to_string()));
        }
        let line = serde_json::to_string(&value).unwrap_or_default();
        println!("{line}");
    }
    if let Err(error) = stream.finish().await {
        emit_error(&error.to_string());
    }
    emit_done();
}

fn event_variant_name(event: &telos_agent::TurnEvent) -> &'static str {
    match event {
        telos_agent::TurnEvent::TurnStarted { .. } => "TurnStarted",
        telos_agent::TurnEvent::IterationStarted { .. } => "IterationStarted",
        telos_agent::TurnEvent::ProviderRequest { .. } => "ProviderRequest",
        telos_agent::TurnEvent::ProviderUsage { .. } => "ProviderUsage",
        telos_agent::TurnEvent::AssistantDelta { .. } => "AssistantDelta",
        telos_agent::TurnEvent::ThinkingDelta { .. } => "ThinkingDelta",
        telos_agent::TurnEvent::User(_) => "User",
        telos_agent::TurnEvent::Assistant(_) => "Assistant",
        telos_agent::TurnEvent::ToolCall { .. } => "ToolCall",
        telos_agent::TurnEvent::ToolProgress { .. } => "ToolProgress",
        telos_agent::TurnEvent::ToolCompleted { .. } => "ToolCompleted",
        telos_agent::TurnEvent::ToolResult(_) => "ToolResult",
        telos_agent::TurnEvent::CompactionStarted { .. } => "CompactionStarted",
        telos_agent::TurnEvent::CompactionCompleted { .. } => "CompactionCompleted",
        telos_agent::TurnEvent::TokenBudgetExceeded { .. } => "TokenBudgetExceeded",
        telos_agent::TurnEvent::PolicyStarted { .. } => "PolicyStarted",
        telos_agent::TurnEvent::PolicyCompleted { .. } => "PolicyCompleted",
        telos_agent::TurnEvent::ApprovalRequested { .. } => "ApprovalRequested",
        telos_agent::TurnEvent::ApprovalResolved { .. } => "ApprovalResolved",
        telos_agent::TurnEvent::ProviderRetry { .. } => "ProviderRetry",
        telos_agent::TurnEvent::TurnFailed { .. } => "TurnFailed",
        telos_agent::TurnEvent::TurnFinished { .. } => "TurnFinished",
    }
}

fn emit_done() {
    println!(r#"{{"type":"_done"}}"#);
}

fn emit_error(message: &str) {
    let escaped = serde_json::to_string(message).unwrap_or_default();
    println!(r#"{{"type":"_error","message":{escaped}}}"#);
    let _ = std::io::stdout().flush();
}

fn emit_event(type_name: &str) {
    println!(r#"{{"type":"{type_name}"}}"#);
    let _ = std::io::stdout().flush();
}
