use std::sync::Arc;

use futures_util::StreamExt;

use crate::compaction::{CompactionConfig, compact_tool_result_message};
use crate::error::AgentError;
use crate::executor::{ToolExecutionEvent, ToolExecutionStreamItem, execute_tool_calls_stream};
use crate::message::Message;
use crate::runtime::{AgentSession, TurnEvent};
use crate::tool::ToolRegistry;

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
        let mut execution = Box::pin(execute_tool_calls_stream(
            tool_calls,
            tools,
            &self.config,
            &self.session_id,
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
                        ToolExecutionEvent::ToolCompleted { tool_call_id, name, is_error } => {
                            TurnEvent::ToolCompleted { tool_call_id, name, is_error }
                        }
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

        for result in &tool_results {
            self.metrics.add_tool_call();
            if result.is_error {
                self.metrics.add_tool_error();
            }
        }

        let tool_message = Message::tool_results(tool_results);
        let compaction_config =
            CompactionConfig { max_tool_result_chars: self.config.max_tool_result_chars };
        let compaction = compact_tool_result_message(tool_message, &compaction_config);
        if compaction.compacted {
            events.push(TurnEvent::CompactionStarted { reason: "tool_result_budget".into() });
            events.push(TurnEvent::CompactionCompleted { reason: "tool_result_budget".into() });
        }

        Ok((compaction.message, events))
    }
}
