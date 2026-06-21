//! `telos serve` — JSON-line daemon mode.
//!
//! Reads one JSON command per line from stdin and emits JSON events per line
//! on stdout. Designed as the backend for a Python Textual TUI.
//!
//! # Commands (stdin, one JSON object per line)
//!
//! | field | type | description |
//! |---|---|---|
//! | `cmd` | string | `"run"`, `"new_session"`, `"quit"` |
//! | `prompt` | string? | user input for `"run"` |
//!
//! # Events (stdout, one JSON object per line)
//!
//! Turn events are serialized with an added `"type"` field. Extra control
//! events: `{"type":"_done"}`, `{"type":"_error","message":"…"}`.

use std::pin::pin;

use anyhow::Result;
use futures_util::StreamExt;
use serde::Deserialize;
use telos_agent::AgentSession;
use tokio::io::AsyncBufReadExt;

use crate::cli::SharedOptions;
use crate::config::FileConfig;

#[derive(Debug, Deserialize)]
struct ServeCommand {
    cmd: String,
    prompt: Option<String>,
}

/// Run the JSON-line daemon.
pub async fn run_serve(options: &SharedOptions, file_config: &FileConfig) -> Result<()> {
    let mut runtime = crate::runtime::prepare_runtime(options, file_config, None)?;
    let provider: std::sync::Arc<dyn telos_agent::ModelProvider> =
        crate::build_erased_provider(options, file_config)?;
    crate::runtime::register_cli_subagent_tool(
        &mut runtime.tools,
        &runtime.agent_config,
        provider.clone(),
    )?;
    crate::runtime::rebuild_prompt_assembly(&mut runtime);

    let base_config = runtime.agent_config.clone();
    let tools = runtime.tools;
    let mut session = AgentSession::new(base_config.clone())?;

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
                run_turn(&mut session, provider.clone(), &tools, prompt).await;
            }
            "new_session" => {
                session = AgentSession::new(base_config.clone())?;
                let _ = session.save().await;
                emit_event("_session_new", None);
            }
            "quit" | "exit" => break,
            other => {
                emit_error(&format!("unknown command: {other}"));
            }
        }
    }

    Ok(())
}

async fn run_turn(
    session: &mut AgentSession,
    provider: std::sync::Arc<dyn telos_agent::ModelProvider>,
    tools: &telos_agent::ToolRegistry,
    prompt: String,
) {
    let erased = telos_agent::ErasedProvider(provider.as_ref());
    let mut stream = pin!(session.run_turn_stream(&erased, tools, prompt));
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => {
                let mut value = serde_json::to_value(&event).unwrap_or_default();
                let type_name = event_variant_name(&event);
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("type".into(), serde_json::Value::String(type_name.to_string()));
                }
                let line = serde_json::to_string(&value).unwrap_or_default();
                println!("{line}");
            }
            Err(e) => {
                emit_error(&e.to_string());
                break;
            }
        }
    }
    // Signal end-of-turn.
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
        telos_agent::TurnEvent::HookStarted { .. } => "HookStarted",
        telos_agent::TurnEvent::HookCompleted { .. } => "HookCompleted",
        telos_agent::TurnEvent::ApprovalRequested { .. } => "ApprovalRequested",
        telos_agent::TurnEvent::ApprovalResolved { .. } => "ApprovalResolved",
        telos_agent::TurnEvent::ProviderRetry { .. } => "ProviderRetry",
        telos_agent::TurnEvent::TurnFinished { .. } => "TurnFinished",
    }
}

fn emit_done() {
    println!(r#"{{"type":"_done"}}"#);
}

fn emit_error(message: &str) {
    let escaped = serde_json::to_string(message).unwrap_or_default();
    println!(r#"{{"type":"_error","message":{escaped}}}"#);
    // Flush so the Python side gets events immediately.
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

fn emit_event(type_name: &str, _payload: Option<&serde_json::Value>) {
    println!(r#"{{"type":"{type_name}"}}"#);
    use std::io::Write;
    let _ = std::io::stdout().flush();
}
