use serde_json::Value;

use crate::error::{AgentError, ProviderError};
use crate::model::message::{ContentBlock, Message, Role, TextBlock, ThinkingBlock, ToolCall};
use crate::model::provider::{CompletionResponse, StopReason, TokenUsage};

use super::types::{DeepSeekFimChoice, DeepSeekFimResponse};

pub(super) fn parse_chat_response(value: Value) -> Result<CompletionResponse, AgentError> {
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| {
            AgentError::Provider(ProviderError::InvalidResponse(
                "provider response missing choice".into(),
            ))
        })?;

    let message_value = choice.get("message").ok_or_else(|| {
        AgentError::Provider(ProviderError::InvalidResponse(
            "provider response missing message".into(),
        ))
    })?;

    let message = parse_assistant_message(message_value)?;
    let stop_reason = parse_stop_reason(choice.get("finish_reason"));
    let usage = value.get("usage").map(parse_usage).transpose()?;

    Ok(CompletionResponse {
        message,
        stop_reason,
        usage,
        model: value.get("model").and_then(Value::as_str).map(str::to_string),
    })
}

pub(super) fn parse_fim_response(value: Value) -> Result<DeepSeekFimResponse, AgentError> {
    let choices = value
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AgentError::Provider(ProviderError::InvalidResponse(
                "FIM response missing choices".into(),
            ))
        })?
        .iter()
        .map(parse_fim_choice)
        .collect::<Result<Vec<_>, _>>()?;

    let usage = value.get("usage").map(parse_usage).transpose()?;

    Ok(DeepSeekFimResponse {
        id: value.get("id").and_then(Value::as_str).map(str::to_string),
        object: value.get("object").and_then(Value::as_str).map(str::to_string),
        created: value.get("created").and_then(Value::as_u64),
        model: value.get("model").and_then(Value::as_str).map(str::to_string),
        choices,
        usage,
    })
}

fn parse_fim_choice(value: &Value) -> Result<DeepSeekFimChoice, AgentError> {
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AgentError::Provider(ProviderError::InvalidResponse("FIM choice missing text".into()))
        })?
        .to_string();
    Ok(DeepSeekFimChoice {
        index: value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize,
        text,
        finish_reason: value.get("finish_reason").and_then(Value::as_str).map(str::to_string),
        logprobs: value.get("logprobs").cloned().filter(|v| !v.is_null()),
    })
}

fn parse_assistant_message(message_value: &Value) -> Result<Message, AgentError> {
    let mut blocks = Vec::new();

    if let Some(reasoning) = message_value
        .get("reasoning_content")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        blocks.push(ContentBlock::Thinking(ThinkingBlock {
            text: reasoning.to_string(),
            signature: None,
            is_redacted: false,
        }));
    }

    if let Some(content) =
        message_value.get("content").and_then(Value::as_str).filter(|text| !text.is_empty())
    {
        blocks.push(ContentBlock::Text(TextBlock { text: content.to_string() }));
    }

    if let Some(tool_calls) = message_value.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            blocks.push(ContentBlock::ToolCall(parse_tool_call(call)?));
        }
    }

    Ok(Message { role: Role::Assistant, blocks })
}

fn parse_tool_call(call: &Value) -> Result<ToolCall, AgentError> {
    let id = call.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    let function = call.get("function").ok_or_else(|| {
        AgentError::Provider(ProviderError::InvalidResponse("tool call missing function".into()))
    })?;
    let name = function.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
    let arguments_text = function.get("arguments").and_then(Value::as_str).unwrap_or("{}");
    let arguments = serde_json::from_str(arguments_text).map_err(|err| {
        AgentError::Provider(ProviderError::InvalidResponse(format!(
            "invalid tool arguments: {err}"
        )))
    })?;
    Ok(ToolCall { id, name, arguments })
}

pub(super) fn parse_stop_reason(value: Option<&Value>) -> StopReason {
    match value.and_then(Value::as_str) {
        Some("tool_calls") => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    }
}

pub(super) fn parse_usage(value: &Value) -> Result<TokenUsage, AgentError> {
    let input_tokens = required_usize(value, "prompt_tokens")?;
    let output_tokens = required_usize(value, "completion_tokens")?;
    Ok(TokenUsage {
        input_tokens,
        output_tokens,
        total_tokens: optional_usize(value, "total_tokens"),
        prompt_cache_hit_tokens: optional_usize(value, "prompt_cache_hit_tokens"),
        prompt_cache_miss_tokens: optional_usize(value, "prompt_cache_miss_tokens"),
        reasoning_tokens: value
            .get("completion_tokens_details")
            .and_then(|details| optional_usize(details, "reasoning_tokens")),
    })
}

fn required_usize(value: &Value, field: &str) -> Result<usize, AgentError> {
    optional_usize(value, field).ok_or_else(|| {
        AgentError::Provider(ProviderError::InvalidResponse(format!("usage missing {field}")))
    })
}

fn optional_usize(value: &Value, field: &str) -> Option<usize> {
    value.get(field).and_then(Value::as_u64).map(|n| n as usize)
}
