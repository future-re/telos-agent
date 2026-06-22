//! Agent turn execution — bridges telos `TurnEvent` stream to `tui_v2` cells.
//!
//! Spawns a background task that runs the agent loop and sends updates
//! through a channel. The app drains the channel on each Tick.

use std::sync::atomic::{AtomicBool, Ordering};

use telos_agent::{AgentSession, ErasedProvider, ModelProvider, ToolRegistry, TurnEvent};
use tokio::sync::mpsc;

use crate::tui::chat::ChatWidget;
use crate::tui::history_cell::{AgentCell, ErrorCell, SeparatorCell, ToolCallCell};

/// Commands sent from the app to the turn runner.
pub enum TurnCommand {
    /// Start a new turn with this prompt.
    Prompt(String),
    /// Cancel the current turn.
    Cancel,
}

/// Events sent from the turn runner to the app.
pub enum TurnUpdate {
    /// Turn started.
    Started,
    /// Push a new cell.
    PushCell(Box<dyn crate::tui::history_cell::HistoryCell>),
    /// Append text to the active agent cell.
    AgentDelta(String),
    /// Append text to the active thinking cell.
    ThinkingDelta(String),
    /// Mark all streaming cells as finished.
    FinishStreaming,
    /// Push a tool call cell.
    ToolCall { id: String, name: String, detail: String },
    /// Update tool call progress.
    ToolProgress { id: String, message: String },
    /// Complete a tool call.
    ToolCompleted { id: String, ok: bool },
    /// Add result content to a tool call.
    ToolResult { id: String, content: serde_json::Value, is_error: bool },
    /// Push a separator.
    PushSeparator,
    /// Update status text.
    StatusText(String),
    /// Turn completed.
    Completed,
    /// Turn errored.
    Error(String),
}

/// Apply a `TurnUpdate` to a `ChatWidget`.
pub fn apply_update(chat: &mut ChatWidget, update: TurnUpdate) {
    match update {
        TurnUpdate::Started => {}
        TurnUpdate::PushCell(cell) => chat.push_cell(cell),
        TurnUpdate::AgentDelta(text) => chat.push_agent_delta(&text),
        TurnUpdate::ThinkingDelta(text) => chat.push_thinking_delta(&text),
        TurnUpdate::FinishStreaming => chat.finish_streaming(),
        TurnUpdate::ToolCall { id, name, detail } => {
            chat.push_cell(Box::new(ToolCallCell::new(id, name, detail)));
        }
        TurnUpdate::ToolProgress { id, message } => {
            if let Some(cell) = chat.find_tool_call_mut(&id)
                && let Some(tool) = cell.as_any_mut().downcast_mut::<ToolCallCell>()
            {
                tool.set_running();
                tool.progress.push(message);
            }
        }
        TurnUpdate::ToolCompleted { id, ok } => {
            if let Some(cell) = chat.find_tool_call_mut(&id)
                && let Some(tool) = cell.as_any_mut().downcast_mut::<ToolCallCell>()
            {
                tool.set_completed(ok);
            }
        }
        TurnUpdate::ToolResult { id, content, is_error } => {
            if let Some(cell) = chat.find_tool_call_mut(&id)
                && let Some(tool) = cell.as_any_mut().downcast_mut::<ToolCallCell>()
            {
                tool.results = crate::tui::tool_rendering::extract_result_lines(&content, is_error);
            }
        }
        TurnUpdate::PushSeparator => {
            chat.push_cell(Box::new(SeparatorCell));
        }
        TurnUpdate::StatusText(_text) => {}
        TurnUpdate::Completed => {
            chat.finish_streaming();
        }
        TurnUpdate::Error(msg) => {
            chat.push_cell(Box::new(ErrorCell { message: msg }));
        }
    }
}

/// Run a turn in the background, sending updates through `tx`.
pub async fn run_turn(
    session: &mut AgentSession,
    provider: &dyn ModelProvider,
    tools: &ToolRegistry,
    prompt: String,
    tx: &mpsc::UnboundedSender<TurnUpdate>,
    cancel_flag: &AtomicBool,
) {
    cancel_flag.store(false, Ordering::Relaxed);

    let _ = tx.send(TurnUpdate::Started);

    let erased = ErasedProvider(provider);
    let mut stream = Box::pin(session.run_turn_stream(&erased, tools, &prompt));

    use futures_util::StreamExt;

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = tx.send(TurnUpdate::FinishStreaming);
            let _ = tx.send(TurnUpdate::Completed);
            return;
        }

        let event = match stream.next().await {
            Some(Ok(e)) => e,
            Some(Err(e)) => {
                let _ = tx.send(TurnUpdate::Error(format!("turn error: {e}")));
                return;
            }
            None => break,
        };

        match event {
            TurnEvent::TurnStarted { .. } => {}

            TurnEvent::ThinkingDelta { text } => {
                let _ = tx.send(TurnUpdate::ThinkingDelta(text));
            }

            TurnEvent::AssistantDelta { text } => {
                let _ = tx.send(TurnUpdate::AgentDelta(text));
            }

            TurnEvent::ToolCall { tool_call_id, name, detail } => {
                // Finish streaming before the tool call cell.
                let _ = tx.send(TurnUpdate::FinishStreaming);
                let _ = tx.send(TurnUpdate::ToolCall {
                    id: tool_call_id.clone(),
                    name: name.clone(),
                    detail: detail.clone(),
                });
                let _ =
                    tx.send(TurnUpdate::StatusText(if detail.is_empty() { name } else { detail }));
            }

            TurnEvent::ToolProgress { tool_call_id: Some(id), message, .. } => {
                let _ = tx.send(TurnUpdate::ToolProgress { id, message });
            }
            TurnEvent::ToolProgress { .. } => {}

            TurnEvent::ToolCompleted { tool_call_id, is_error, .. } => {
                let _ = tx.send(TurnUpdate::ToolCompleted { id: tool_call_id, ok: !is_error });
            }

            TurnEvent::ToolResult(message) => {
                for result in message.tool_results_iter() {
                    let _ = tx.send(TurnUpdate::ToolResult {
                        id: result.tool_call_id.clone(),
                        content: result.content.clone(),
                        is_error: result.is_error,
                    });
                }
            }

            TurnEvent::TurnFinished { final_text, .. } => {
                let _ = tx.send(TurnUpdate::FinishStreaming);
                if !final_text.is_empty() {
                    let _ = tx.send(TurnUpdate::PushCell(Box::new(AgentCell {
                        buffer: final_text,
                        is_streaming: false,
                    })));
                }
                let _ = tx.send(TurnUpdate::Completed);
            }

            TurnEvent::TokenBudgetExceeded { .. } => {
                let _ = tx.send(TurnUpdate::Error("token budget exceeded".into()));
            }

            _ => {}
        }
    }

    let _ = tx.send(TurnUpdate::Completed);
}
