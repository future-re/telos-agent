//! Turn events and result types for the agent runtime.

use crate::error::AgentError;
use crate::message::{ContentBlock, Message};
use crate::provider::StopReason;
use serde::Serialize;

/// Streaming event emitted during a single turn of the agent loop.
///
/// Events are emitted in causal order — e.g. an [`AssistantDelta`](Self::AssistantDelta)
/// for each streamed text fragment, then [`Assistant`](Self::Assistant) once
/// the full message is materialised, then per-tool events if the model
/// requested tool calls.
#[derive(Debug, Clone, Serialize)]
pub enum TurnEvent {
    /// Fired exactly once at the start of a turn with the user's input.
    TurnStarted { session_id: String, turn_id: u64, user_input: String },
    /// Fired at the top of each model ⇄ tool iteration within the turn.
    IterationStarted { iteration: usize, message_count: usize },
    /// About to issue a completion request to the provider.
    ProviderRequest { iteration: usize, message_count: usize, tool_count: usize },
    /// Provider reported token usage for the just-finished iteration.
    ProviderUsage {
        input_tokens: usize,
        output_tokens: usize,
        total_tokens: Option<usize>,
        prompt_cache_hit_tokens: Option<usize>,
        prompt_cache_miss_tokens: Option<usize>,
        reasoning_tokens: Option<usize>,
        model: Option<String>,
    },
    /// Incremental text fragment streamed from the assistant.
    AssistantDelta { text: String },
    /// Incremental reasoning fragment streamed from a thinking-capable model.
    ThinkingDelta { text: String },
    /// The full user message that was just appended to the conversation.
    User(Message),
    /// A completed assistant message (either model output or hook-emitted).
    Assistant(Message),
    /// A tool call has begun executing.
    ToolCall { tool_call_id: String, name: String, detail: String },
    /// Progress update emitted from inside a long-running tool.
    ToolProgress {
        tool_call_id: Option<String>,
        name: String,
        message: String,
        data: Option<serde_json::Value>,
    },
    /// A tool call finished (successfully or with an error).
    ToolCompleted { tool_call_id: String, name: String, is_error: bool, detail: Option<String> },
    /// The aggregated tool-result message appended to the conversation.
    ToolResult(Message),
    /// A compaction pass is starting; `reason` identifies which threshold tripped.
    CompactionStarted { reason: String },
    /// A compaction pass finished.
    CompactionCompleted { reason: String },
    /// Estimated request size exceeded [`TokenBudget::max_tokens`](crate::TokenBudget::max_tokens);
    /// the turn ends without calling the model.
    TokenBudgetExceeded { used_tokens: usize, max_tokens: usize },
    /// A registered hook is starting.
    HookStarted { phase: String, name: String },
    /// A registered hook finished; `emitted_message` is `true` if it appended a follow-up.
    HookCompleted { phase: String, name: String, emitted_message: bool },
    /// A tool call has been suspended pending human approval.
    ApprovalRequested { tool_call_id: String, name: String, reason: String },
    /// Human approval has been resolved for a suspended tool call.
    ApprovalResolved { tool_call_id: String, name: String, decision: String },
    /// A provider call failed with a retryable error and is being retried.
    ProviderRetry { attempt: usize, max_retries: usize, delay_ms: u64 },
    /// Final event of a turn — the assistant produced an end-of-turn message.
    TurnFinished { stop_reason: StopReason, final_text: String },
}

/// Collected result of a turn, returned by [`AgentSession::run_turn`](crate::AgentSession::run_turn).
#[derive(Debug, Clone, Serialize)]
pub struct TurnResult {
    /// Every event emitted during the turn, in order.
    pub events: Vec<TurnEvent>,
    /// The last assistant message seen (the answer the caller usually wants).
    pub final_message: Message,
    /// Why the turn stopped — informational for callers.
    pub stop_reason: StopReason,
    /// If session persistence was configured but failed, the error is surfaced
    /// here so callers can react without losing the in-memory turn result.
    pub save_error: Option<AgentError>,
}

impl TurnEvent {
    /// Return the [`Message`] carried by this event, if any.
    ///
    /// Only [`User`](TurnEvent::User), [`Assistant`](TurnEvent::Assistant), and
    /// [`ToolResult`](TurnEvent::ToolResult) carry messages.
    pub fn message(&self) -> Option<&Message> {
        match self {
            TurnEvent::User(message)
            | TurnEvent::Assistant(message)
            | TurnEvent::ToolResult(message) => Some(message),
            _ => None,
        }
    }

    /// Human-readable one-line summary of the event — useful for trace logs / CLIs.
    pub fn text(&self) -> String {
        match self {
            TurnEvent::TurnStarted { session_id, turn_id, user_input } => {
                format!("turn_started:{}#{}:{}", session_id, turn_id, user_input)
            }
            TurnEvent::IterationStarted { iteration, message_count } => {
                format!("iteration_started:{} messages={}", iteration, message_count)
            }
            TurnEvent::ProviderRequest { iteration, message_count, tool_count } => format!(
                "provider_request:{} messages={} tools={}",
                iteration, message_count, tool_count
            ),
            TurnEvent::ProviderUsage {
                input_tokens,
                output_tokens,
                total_tokens,
                prompt_cache_hit_tokens,
                prompt_cache_miss_tokens,
                reasoning_tokens,
                model,
            } => {
                let mut parts =
                    vec![format!("input={input_tokens}"), format!("output={output_tokens}")];
                if let Some(tokens) = total_tokens {
                    parts.push(format!("total={tokens}"));
                }
                if let Some(tokens) = prompt_cache_hit_tokens {
                    parts.push(format!("cache_hit={tokens}"));
                }
                if let Some(tokens) = prompt_cache_miss_tokens {
                    parts.push(format!("cache_miss={tokens}"));
                }
                if let Some(tokens) = reasoning_tokens {
                    parts.push(format!("reasoning={tokens}"));
                }
                if let Some(model) = model {
                    parts.push(format!("model={model}"));
                }
                format!("provider_usage:{}", parts.join(" "))
            }
            TurnEvent::AssistantDelta { text } => format!("assistant_delta:{text}"),
            TurnEvent::ThinkingDelta { text } => format!("thinking_delta:{text}"),
            TurnEvent::ToolCall { tool_call_id, name, detail } => {
                if detail.is_empty() {
                    format!("tool_call:{}#{}", name, tool_call_id)
                } else {
                    format!("tool_call:{}#{} {}", name, tool_call_id, detail)
                }
            }
            TurnEvent::ToolProgress { tool_call_id, name, message, .. } => format!(
                "tool_progress:{}#{}:{}",
                name,
                tool_call_id.as_deref().unwrap_or("unknown"),
                message
            ),
            TurnEvent::ToolCompleted { tool_call_id, name, is_error, detail } => {
                if let Some(detail) = detail {
                    format!(
                        "tool_completed:{}#{} error={} {}",
                        name, tool_call_id, is_error, detail
                    )
                } else {
                    format!("tool_completed:{}#{} error={}", name, tool_call_id, is_error)
                }
            }
            TurnEvent::CompactionStarted { reason } => {
                format!("compaction_started:{reason}")
            }
            TurnEvent::CompactionCompleted { reason } => {
                format!("compaction_completed:{reason}")
            }
            TurnEvent::TokenBudgetExceeded { used_tokens, max_tokens } => {
                format!("token_budget_exceeded:{used_tokens}/{max_tokens}")
            }
            TurnEvent::HookStarted { phase, name } => {
                format!("hook_started:{phase}:{name}")
            }
            TurnEvent::HookCompleted { phase, name, emitted_message } => {
                format!("hook_completed:{phase}:{name}:{emitted_message}")
            }
            TurnEvent::ApprovalRequested { tool_call_id, name, reason } => {
                format!("approval_requested:{name}#{tool_call_id}:{reason}")
            }
            TurnEvent::ApprovalResolved { tool_call_id, name, decision } => {
                format!("approval_resolved:{name}#{tool_call_id}:{decision}")
            }
            TurnEvent::ProviderRetry { attempt, max_retries, delay_ms } => {
                format!("provider_retry:{attempt}/{max_retries} delay={delay_ms}ms")
            }
            TurnEvent::TurnFinished { stop_reason, final_text } => {
                format!("turn_finished:{stop_reason:?}:{final_text}")
            }
            _ => self
                .message()
                .map(|message| {
                    message
                        .blocks
                        .iter()
                        .map(|block| match block {
                            ContentBlock::Text(text) => text.text.clone(),
                            ContentBlock::Thinking(thinking) => {
                                format!("thinking:{}", thinking.text)
                            }
                            ContentBlock::ToolCall(call) => {
                                format!("tool_call:{}({})", call.name, call.arguments)
                            }
                            ContentBlock::ToolResult(result) => {
                                format!("tool_result:{}={}", result.name, result.content)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn turn_event_message_returns_some_for_message_variants() {
        let message = Message::user("hi");
        assert!(matches!(TurnEvent::User(message.clone()).message(), Some(m) if m == &message));
        assert!(
            TurnEvent::TurnStarted { session_id: "s".into(), turn_id: 1, user_input: "hi".into() }
                .message()
                .is_none()
        );
    }
}
