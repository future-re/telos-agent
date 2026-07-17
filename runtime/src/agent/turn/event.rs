use crate::model::message::{ContentBlock, Message};
use crate::model::provider::StopReason;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum TurnEvent {
    TurnStarted {
        session_id: String,
        turn_id: u64,
        user_input: String,
    },
    IterationStarted {
        iteration: usize,
        message_count: usize,
    },
    ProviderRequest {
        iteration: usize,
        message_count: usize,
        tool_count: usize,
    },
    ProviderUsage {
        input_tokens: usize,
        output_tokens: usize,
        total_tokens: Option<usize>,
        prompt_cache_hit_tokens: Option<usize>,
        prompt_cache_miss_tokens: Option<usize>,
        reasoning_tokens: Option<usize>,
        model: Option<String>,
    },
    AssistantDelta {
        text: String,
    },
    ThinkingDelta {
        text: String,
    },
    User(Message),
    Assistant(Message),
    ToolCall {
        tool_call_id: String,
        name: String,
        detail: String,
    },
    ToolProgress {
        tool_call_id: Option<String>,
        name: String,
        message: String,
        data: Option<serde_json::Value>,
    },
    ToolCompleted {
        tool_call_id: String,
        name: String,
        is_error: bool,
        detail: Option<String>,
    },
    ToolResult(Message),
    CompactionStarted {
        reason: String,
    },
    CompactionCompleted {
        reason: String,
    },
    TokenBudgetExceeded {
        used_tokens: usize,
        max_tokens: usize,
    },
    PolicyStarted {
        point: String,
        name: String,
    },
    PolicyCompleted {
        point: String,
        name: String,
        feedback_count: usize,
    },
    ApprovalRequested {
        tool_call_id: String,
        name: String,
        reason: String,
    },
    ApprovalResolved {
        tool_call_id: String,
        name: String,
        decision: String,
    },
    ProviderRetry {
        attempt: usize,
        max_retries: usize,
        delay_ms: u64,
    },
    TurnFailed {
        error: String,
    },
    TurnFinished {
        stop_reason: StopReason,
        final_text: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnResult {
    pub events: Vec<TurnEvent>,
    pub final_message: Message,
    pub stop_reason: StopReason,
}

impl TurnEvent {
    pub fn message(&self) -> Option<&Message> {
        match self {
            TurnEvent::User(message)
            | TurnEvent::Assistant(message)
            | TurnEvent::ToolResult(message) => Some(message),
            _ => None,
        }
    }

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
            TurnEvent::CompactionStarted { reason } => format!("compaction_started:{reason}"),
            TurnEvent::CompactionCompleted { reason } => format!("compaction_completed:{reason}"),
            TurnEvent::TokenBudgetExceeded { used_tokens, max_tokens } => {
                format!("token_budget_exceeded:{used_tokens}/{max_tokens}")
            }
            TurnEvent::PolicyStarted { point, name } => {
                format!("policy_started:{point}:{name}")
            }
            TurnEvent::PolicyCompleted { point, name, feedback_count } => {
                format!("policy_completed:{point}:{name}:{feedback_count}")
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
            TurnEvent::TurnFailed { error } => format!("turn_failed:{error}"),
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
                            ContentBlock::ToolResult(result) => format!(
                                "tool_result:{}#{} error={}",
                                result.name, result.tool_call_id, result.is_error
                            ),
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
