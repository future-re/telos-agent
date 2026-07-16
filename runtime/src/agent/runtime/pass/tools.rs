use std::sync::Arc;

use futures_util::StreamExt;

use crate::agent::compaction::{MessageTruncationConfig, truncate_message};
use crate::agent::context::Conversation;
use crate::agent::turn::TurnEvent;
use crate::error::AgentError;
use crate::model::message::Message;
use crate::tools::api::ToolRegistry;
use crate::tools::executor::{
    ToolExecutionEvent, ToolExecutionStreamItem, execute_tool_calls_stream, tool_result_detail,
};
use tracing::debug;

use super::super::{session::SessionInfo, state::RuntimeState};

/// Executes a batch of tool calls for the given turn.
///
/// Drives an execution stream to completion, translating each lifecycle
/// event (start, progress, approval, result) into [TurnEvent]s. After
/// all calls finish, tool results are packed into a single [Message]
/// that may be truncated if it exceeds the configured size budget.
///
/// # Errors
/// Returns `AgentError::Cancelled` if the session's cancellation token
/// fires before execution finishes.
pub(super) async fn execute_tool_calls_phase(
    session: &mut SessionInfo,
    context: &mut Conversation,
    state: &mut RuntimeState,
    tools: &ToolRegistry,
    tool_calls: Vec<crate::model::message::ToolCall>,
    turn_id: u64,
) -> Result<(Message, Vec<TurnEvent>), AgentError> {
    let mut events = Vec::new();
    // Snapshot messages and config so the execution stream can reference them
    // without borrowing `context` or `session` for its entire lifetime.
    let messages = Arc::new(context.messages().to_vec());
    let config = session.config().clone();
    let session_id = session.session_id().to_string();
    let mut execution = Box::pin(execute_tool_calls_stream(
        tool_calls,
        tools,
        &config,
        &session_id,
        turn_id,
        messages,
        state.read_file_state().clone(),
    ));

    let mut tool_results = Vec::new();
    let cancellation = session.config().cancellation.clone();
    // Drive the stream, cancelling on token signal.
    while let Some(item) = tokio::select! {
        _ = cancellation.cancelled() => return Err(AgentError::Cancelled),
        item = execution.next() => item,
    } {
        match item {
            // Non-terminal lifecycle events → forward as-is.
            ToolExecutionStreamItem::Event(event) => {
                let turn_event = match event {
                    ToolExecutionEvent::ToolStarted { tool_call_id, name, detail } => {
                        TurnEvent::ToolCall { tool_call_id, name, detail }
                    }
                    ToolExecutionEvent::ToolProgress { tool_call_id, name, message, data } => {
                        TurnEvent::ToolProgress { tool_call_id, name, message, data }
                    }
                    ToolExecutionEvent::ApprovalRequested { tool_call_id, name, reason } => {
                        TurnEvent::ApprovalRequested { tool_call_id, name, reason }
                    }
                    ToolExecutionEvent::ApprovalResolved { tool_call_id, name, decision } => {
                        TurnEvent::ApprovalResolved { tool_call_id, name, decision }
                    }
                };
                session.emit_turn_event(&turn_event);
                events.push(turn_event);
            }
            // Terminal result → record as ToolCompleted and retain for the result message.
            ToolExecutionStreamItem::Result(result) => {
                let event = TurnEvent::ToolCompleted {
                    tool_call_id: result.tool_call_id.clone(),
                    name: result.name.clone(),
                    is_error: result.is_error,
                    detail: result.is_error.then(|| tool_result_detail(&result.content)),
                };
                session.emit_turn_event(&event);
                events.push(event);
                tool_results.push(result);
            }
        }
    }

    // Collect names of successful memory-mutating tools so we can flag the
    // memory state as dirty below.
    let memory_mutations: Vec<String> = tool_results
        .iter()
        .filter(|r| !r.is_error && &r.name == "MemoryWrite" || &r.name == "MemoryEdit")
        .map(|r| r.name.clone())
        .collect();

    // Track per-turn metrics.
    for result in &tool_results {
        state.metrics_mut().add_tool_call();
        if result.is_error {
            state.metrics_mut().add_tool_error();
        }
    }

    // Build the tool-results message and apply size-budget truncation.
    let tool_message = Message::tool_results(tool_results);
    let truncation_config = MessageTruncationConfig {
        max_block_content_bytes: Some(session.config().max_tool_result_chars),
        max_message_bytes: Some(session.config().max_message_tool_results_chars),
        compressor: None,
    };
    let truncation = truncate_message(tool_message, &truncation_config);
    if truncation.compacted {
        let started = TurnEvent::CompactionStarted { reason: "tool_result_budget".into() };
        let completed = TurnEvent::CompactionCompleted { reason: "tool_result_budget".into() };
        session.emit_turn_event(&started);
        session.emit_turn_event(&completed);
        events.push(started);
        events.push(completed);
    }

    // If any memory tool wrote or edited state, mark context dirty and, if
    // the model hasn't already been notified this turn, push a system reminder
    // so it can request fresh memory on the next reasoning step.
    if !memory_mutations.is_empty() {
        context.set_memory_dirty(true);
        debug!(
            session_id = %session.session_id(),
            turn_id,
            memory_mutation_count = memory_mutations.len(),
            "memory state marked dirty"
        );
        if !context.turn_memory_injected() && !context.turn_memory_mutation_notified() {
            debug!(
                session_id = %session.session_id(),
                turn_id,
                "emitting memory mutation reminder"
            );
            context.push_system_reminder(crate::model::message::SystemReminder::ToolResult {
            tool_name: "memory".into(),
            note: "Memory state changed this turn. Fresh memory context is available for future reasoning.".into(),
        });
            context.set_turn_memory_mutation_notified(true);
        }
    }

    Ok((truncation.message, events))
}
