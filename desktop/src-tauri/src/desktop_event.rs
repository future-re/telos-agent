use serde::Serialize;
use telos_agent::TurnEvent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopEvent {
    pub kind: String,
    pub session_id: Option<String>,
    pub text: Option<String>,
    pub input_tokens: Option<usize>,
    pub output_tokens: Option<usize>,
    pub total_tokens: Option<usize>,
    pub prompt_cache_hit_tokens: Option<usize>,
    pub prompt_cache_miss_tokens: Option<usize>,
    pub reasoning_tokens: Option<usize>,
    pub model: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub detail: Option<String>,
    pub is_error: Option<bool>,
    pub message: Option<String>,
}

impl DesktopEvent {
    fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            session_id: None,
            text: None,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            reasoning_tokens: None,
            model: None,
            tool_call_id: None,
            tool_name: None,
            detail: None,
            is_error: None,
            message: None,
        }
    }
}

pub fn map_turn_event(session_id: &str, event: TurnEvent) -> DesktopEvent {
    match event {
        TurnEvent::AssistantDelta { text } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            text: Some(text),
            ..DesktopEvent::new("assistant_delta")
        },
        TurnEvent::ThinkingDelta { text } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            text: Some(text),
            ..DesktopEvent::new("thinking_delta")
        },
        TurnEvent::ToolCall { tool_call_id, name, detail } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            tool_call_id: Some(tool_call_id),
            tool_name: Some(name),
            detail: Some(detail),
            ..DesktopEvent::new("tool_call")
        },
        TurnEvent::ToolProgress { tool_call_id, message, .. } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            tool_call_id,
            message: Some(message),
            ..DesktopEvent::new("tool_progress")
        },
        TurnEvent::ToolCompleted { tool_call_id, name, is_error, detail } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            tool_call_id: Some(tool_call_id),
            tool_name: Some(name),
            detail,
            is_error: Some(is_error),
            ..DesktopEvent::new("tool_completed")
        },
        TurnEvent::ApprovalRequested { tool_call_id, name, reason } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            tool_call_id: Some(tool_call_id),
            tool_name: Some(name),
            message: Some(reason),
            ..DesktopEvent::new("approval_requested")
        },
        TurnEvent::ApprovalResolved { tool_call_id, name, decision } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            tool_call_id: Some(tool_call_id),
            tool_name: Some(name),
            message: Some(decision),
            ..DesktopEvent::new("approval_resolved")
        },
        TurnEvent::ProviderUsage {
            input_tokens,
            output_tokens,
            total_tokens,
            prompt_cache_hit_tokens,
            prompt_cache_miss_tokens,
            reasoning_tokens,
            model,
        } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            total_tokens,
            prompt_cache_hit_tokens,
            prompt_cache_miss_tokens,
            reasoning_tokens,
            model,
            ..DesktopEvent::new("provider_usage")
        },
        TurnEvent::TurnFinished { final_text, .. } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            text: Some(final_text),
            ..DesktopEvent::new("turn_finished")
        },
        TurnEvent::ProviderRetry { attempt, max_retries, delay_ms } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            message: Some(format!("retrying ({attempt}/{max_retries}, {delay_ms}ms)")),
            ..DesktopEvent::new("provider_retry")
        },
        TurnEvent::TokenBudgetExceeded { used_tokens, max_tokens } => DesktopEvent {
            session_id: Some(session_id.to_string()),
            message: Some(format!("token budget exceeded: {used_tokens}/{max_tokens}")),
            is_error: Some(true),
            ..DesktopEvent::new("token_budget_exceeded")
        },
        _ => DesktopEvent::new("ignored"),
    }
}

#[cfg(test)]
mod tests {
    use telos_agent::TurnEvent;

    use super::*;

    #[test]
    fn maps_assistant_delta_for_frontend() {
        let event = map_turn_event("session-1", TurnEvent::AssistantDelta { text: "hello".into() });

        assert_eq!(event.kind, "assistant_delta");
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(event.text.as_deref(), Some("hello"));
    }

    #[test]
    fn maps_provider_usage_for_frontend() {
        let event = map_turn_event(
            "session-1",
            TurnEvent::ProviderUsage {
                input_tokens: 10,
                output_tokens: 4,
                total_tokens: Some(14),
                prompt_cache_hit_tokens: Some(3),
                prompt_cache_miss_tokens: None,
                reasoning_tokens: Some(2),
                model: Some("deepseek-v4-flash".into()),
            },
        );

        assert_eq!(event.kind, "provider_usage");
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(event.input_tokens, Some(10));
        assert_eq!(event.output_tokens, Some(4));
        assert_eq!(event.total_tokens, Some(14));
        assert_eq!(event.prompt_cache_hit_tokens, Some(3));
        assert_eq!(event.reasoning_tokens, Some(2));
        assert_eq!(event.model.as_deref(), Some("deepseek-v4-flash"));
    }
}
