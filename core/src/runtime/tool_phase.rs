use std::sync::Arc;

use futures_util::StreamExt;

use crate::compaction::{MessageTruncationConfig, truncate_message};
use crate::error::AgentError;
use crate::executor::{ToolExecutionEvent, ToolExecutionStreamItem, execute_tool_calls_stream};
use crate::message::Message;
use crate::runtime::{AgentSession, TurnEvent};
use crate::tool::ToolRegistry;
use tracing::debug;

impl AgentSession {
    /// Execute a batch of tool calls and build the compacted tool-result message.
    pub(super) async fn execute_tool_calls_phase(
        &mut self,
        tools: &ToolRegistry,
        tool_calls: Vec<crate::message::ToolCall>,
        turn_id: u64,
    ) -> Result<(Message, Vec<TurnEvent>), AgentError> {
        let mut events = Vec::new();
        let messages = Arc::new(self.messages.clone());
        // Clone config/session_id to release the immutable borrow on self
        // before we push system-reminders later (which needs &mut self).
        let config = self.config.clone();
        let session_id = self.session_id.clone();
        let mut execution = Box::pin(execute_tool_calls_stream(
            tool_calls,
            tools,
            &config,
            &session_id,
            turn_id,
            messages,
            self.read_file_state.clone(),
        ));

        let mut tool_results = Vec::new();
        let cancellation = self.config.cancellation.clone();
        while let Some(item) = tokio::select! {
            _ = cancellation.cancelled() => return Err(AgentError::Cancelled),
            item = execution.next() => item,
        } {
            match item {
                ToolExecutionStreamItem::Event(event) => {
                    let turn_event = match event {
                        ToolExecutionEvent::ToolStarted { tool_call_id, name, detail } => {
                            TurnEvent::ToolCall { tool_call_id, name, detail }
                        }
                        ToolExecutionEvent::ToolProgress { tool_call_id, name, message, data } => {
                            TurnEvent::ToolProgress { tool_call_id, name, message, data }
                        }
                        ToolExecutionEvent::ToolCompleted {
                            tool_call_id,
                            name,
                            is_error,
                            detail,
                        } => TurnEvent::ToolCompleted { tool_call_id, name, is_error, detail },
                        ToolExecutionEvent::ApprovalRequested { tool_call_id, name, reason } => {
                            TurnEvent::ApprovalRequested { tool_call_id, name, reason }
                        }
                        ToolExecutionEvent::ApprovalResolved { tool_call_id, name, decision } => {
                            TurnEvent::ApprovalResolved { tool_call_id, name, decision }
                        }
                    };
                    events.push(turn_event);
                }
                ToolExecutionStreamItem::Result(result) => {
                    tool_results.push(result);
                }
            }
        }

        // Collect memory-mutation tool names before tool_results is moved
        // into the Message builder. We'll push system reminders afterwards
        // so the model knows its memory state may be stale.
        let memory_mutations: Vec<String> = tool_results
            .iter()
            .filter(|r| !r.is_error && is_memory_mutation_tool(&r.name))
            .map(|r| r.name.clone())
            .collect();

        for result in &tool_results {
            self.metrics.add_tool_call();
            if result.is_error {
                self.metrics.add_tool_error();
            }
        }

        let tool_message = Message::tool_results(tool_results);
        let truncation_config = MessageTruncationConfig {
            max_block_content_bytes: Some(self.config.max_tool_result_chars),
            max_message_bytes: Some(self.config.max_message_tool_results_chars),
            compressor: None,
        };
        let truncation = truncate_message(tool_message, &truncation_config);
        if truncation.compacted {
            events.push(TurnEvent::CompactionStarted { reason: "tool_result_budget".into() });
            events.push(TurnEvent::CompactionCompleted { reason: "tool_result_budget".into() });
        }

        // Notify the model when persistent state it may rely on has changed.
        // MemorySection is Static (rendered once at session start), so new
        // memories must be injected mid-conversation via system reminders.
        if !memory_mutations.is_empty() {
            self.memory_state_dirty = true;
            debug!(
                session_id = %self.session_id,
                turn_id,
                memory_mutation_count = memory_mutations.len(),
                "memory state marked dirty"
            );
        }

        if !memory_mutations.is_empty()
            && !self.current_turn_memory_injected
            && !self.current_turn_memory_mutation_notified
        {
            debug!(
                session_id = %self.session_id,
                turn_id,
                "emitting memory mutation reminder"
            );
            self.push_system_reminder(crate::message::SystemReminder::ToolResult {
                tool_name: "memory".into(),
                note: "Memory state changed this turn. Fresh memory context is available for future reasoning.".into(),
            });
            self.current_turn_memory_mutation_notified = true;
        }

        Ok((truncation.message, events))
    }
}

/// Tools that mutate the memory store. When one of these succeeds, the
/// runtime injects a system reminder so the model knows its memory state
/// is stale and fresh results are available.
fn is_memory_mutation_tool(tool_name: &str) -> bool {
    matches!(tool_name, "MemoryWrite" | "MemoryEdit" | "memory_write" | "memory_edit")
}
