use std::time::Instant;

use crate::tui::history_cell::{AgentCell, ErrorCell};
use telos_agent::TurnEvent;

use super::App;

impl App {
    pub(super) async fn handle_turn_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::TurnStarted { .. } => {
                self.status_text = "thinking…".to_string();
                self.turn_started = Some(Instant::now());
                self.reset_turn_usage();
                self.turn_tool_calls = 0;
                self.turn_tool_failures = 0;
            }
            TurnEvent::AssistantDelta { text } => {
                self.status_text = "streaming…".to_string();
                self.chat.push_agent_delta(&text);
            }
            TurnEvent::ThinkingDelta { text } => {
                self.chat.push_thinking_delta(&text);
            }
            TurnEvent::ToolCall { tool_call_id, name, detail } => {
                let label = if detail.is_empty() { name.clone() } else { detail.clone() };
                self.status_text = label;
                self.turn_tool_calls = self.turn_tool_calls.saturating_add(1);
                self.tool_activity.push_call(tool_call_id, name, detail);
            }
            TurnEvent::ToolProgress { tool_call_id, message, .. } => {
                if !message.starts_with("running command with") {
                    self.status_text = message.to_string();
                }
                if let Some(ref id) = tool_call_id {
                    self.tool_activity.set_progress(id, message);
                }
            }
            TurnEvent::ToolCompleted { tool_call_id, name, is_error, .. } => {
                let detail = self.tool_activity.complete(&tool_call_id, name.clone(), !is_error);
                if is_error {
                    self.turn_tool_failures = self.turn_tool_failures.saturating_add(1);
                }

                if !is_error {
                    crate::memory_runtime::record_successful_tool(
                        &self.memory,
                        &name,
                        &tool_call_id,
                        Some(&detail),
                    )
                    .await;
                }
            }
            TurnEvent::ToolResult(message) => {
                for result in message.tool_results_iter() {
                    crate::memory_runtime::record_subagent_learning(&self.memory, result).await;
                    self.tool_activity.add_result_content(
                        &result.tool_call_id,
                        &result.content,
                        result.is_error,
                    );
                    if result.is_error {
                        crate::memory_runtime::record_tool_error(&self.memory, result, None).await;
                    }
                }
            }
            TurnEvent::TurnFinished { final_text, .. } => {
                let had_streamed_assistant = self.chat.has_active_assistant();
                self.chat.finish_streaming_cells();
                if !final_text.is_empty() && !had_streamed_assistant {
                    self.chat
                        .push_cell(Box::new(AgentCell { buffer: final_text, is_streaming: false }));
                }
            }
            TurnEvent::TokenBudgetExceeded { used_tokens, max_tokens } => {
                self.chat.push_cell(Box::new(ErrorCell {
                    message: format!("token budget exceeded: {used_tokens}/{max_tokens}"),
                }));
            }
            TurnEvent::ProviderRetry { attempt, max_retries, delay_ms } => {
                self.status_text = format!("retrying ({attempt}/{max_retries}, {delay_ms}ms)");
            }
            TurnEvent::ProviderUsage {
                input_tokens,
                output_tokens,
                total_tokens,
                prompt_cache_hit_tokens,
                prompt_cache_miss_tokens,
                reasoning_tokens,
            } => {
                self.turn_input_tokens = input_tokens as u64;
                self.turn_output_tokens = output_tokens as u64;
                self.turn_total_tokens = total_tokens.map(|tokens| tokens as u64);
                self.turn_prompt_cache_hit_tokens =
                    prompt_cache_hit_tokens.map(|tokens| tokens as u64);
                self.turn_prompt_cache_miss_tokens =
                    prompt_cache_miss_tokens.map(|tokens| tokens as u64);
                self.turn_reasoning_tokens = reasoning_tokens.map(|tokens| tokens as u64);
                self.turn_has_provider_usage = true;
            }
            TurnEvent::ApprovalRequested { tool_call_id, name, reason } => {
                self.tool_activity.approval_requested(&tool_call_id, name, reason);
            }
            TurnEvent::ApprovalResolved { tool_call_id, name, decision } => {
                self.tool_activity.approval_resolved(&tool_call_id, name, decision);
            }
            _ => {}
        }
    }
}
