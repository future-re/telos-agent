use serde::Serialize;
use telos_agent::TurnEvent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopEvent {
    pub kind: String,
    pub text: Option<String>,
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
            text: None,
            tool_call_id: None,
            tool_name: None,
            detail: None,
            is_error: None,
            message: None,
        }
    }
}

pub fn map_turn_event(event: TurnEvent) -> DesktopEvent {
    match event {
        TurnEvent::AssistantDelta { text } => {
            DesktopEvent { text: Some(text), ..DesktopEvent::new("assistant_delta") }
        }
        TurnEvent::ThinkingDelta { text } => {
            DesktopEvent { text: Some(text), ..DesktopEvent::new("thinking_delta") }
        }
        TurnEvent::ToolCall { tool_call_id, name, detail } => DesktopEvent {
            tool_call_id: Some(tool_call_id),
            tool_name: Some(name),
            detail: Some(detail),
            ..DesktopEvent::new("tool_call")
        },
        TurnEvent::ToolProgress { tool_call_id, message, .. } => DesktopEvent {
            tool_call_id,
            message: Some(message),
            ..DesktopEvent::new("tool_progress")
        },
        TurnEvent::ToolCompleted { tool_call_id, name, is_error } => DesktopEvent {
            tool_call_id: Some(tool_call_id),
            tool_name: Some(name),
            is_error: Some(is_error),
            ..DesktopEvent::new("tool_completed")
        },
        TurnEvent::TurnFinished { final_text, .. } => {
            DesktopEvent { text: Some(final_text), ..DesktopEvent::new("turn_finished") }
        }
        TurnEvent::ProviderRetry { attempt, max_retries, delay_ms } => DesktopEvent {
            message: Some(format!("retrying ({attempt}/{max_retries}, {delay_ms}ms)")),
            ..DesktopEvent::new("provider_retry")
        },
        TurnEvent::TokenBudgetExceeded { used_tokens, max_tokens } => DesktopEvent {
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
        let event = map_turn_event(TurnEvent::AssistantDelta { text: "hello".into() });

        assert_eq!(event.kind, "assistant_delta");
        assert_eq!(event.text.as_deref(), Some("hello"));
    }
}
