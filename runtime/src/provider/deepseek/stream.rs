use async_stream::try_stream;
use futures_core::stream::Stream;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::error::{AgentError, ProviderError};
use crate::message::ToolCall;
use crate::provider::{ProviderEvent, StopReason, TokenUsage};

use super::error::{classify_reqwest_error, map_deepseek_http_error};
use super::response::{parse_stop_reason, parse_usage};

pub(super) fn stream_json_to<'a>(
    client: &'a reqwest::Client,
    api_key: &'a str,
    url: String,
    body: Value,
) -> impl Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a {
    try_stream! {
        let response = send_stream_json(client, api_key, &url, body).await?;
        let mut byte_stream = response.bytes_stream();
        let mut pending = String::new();
        let mut tool_calls: Vec<StreamingToolCall> = Vec::new();
        let mut stop_reason = StopReason::EndTurn;
        let mut usage = None;
        let mut model = None;

        yield ProviderEvent::MessageStart;

        while let Some(chunk) = byte_stream.next().await {
            let chunk = chunk.map_err(classify_reqwest_error)?;
            pending.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline_idx) = pending.find('\n') {
                let line = pending[..newline_idx].trim_end_matches('\r').to_string();
                pending.drain(..=newline_idx);
                if let Some(data) = line.strip_prefix("data: ") {
                    if data.trim() == "[DONE]" {
                        continue;
                    }
                    let value: Value = serde_json::from_str(data).map_err(|err| {
                        AgentError::Provider(ProviderError::InvalidResponse(format!(
                            "invalid stream chunk: {err}"
                        )))
                    })?;
                    for event in apply_stream_chunk(
                        value,
                        &mut tool_calls,
                        &mut stop_reason,
                        &mut usage,
                        &mut model,
                    )? {
                        yield event;
                    }
                }
            }
        }

        for call in tool_calls {
            if call.id.is_empty() || call.name.is_empty() {
                Err(AgentError::Provider(ProviderError::InvalidResponse(
                    "streamed tool call missing id or name".into(),
                )))?;
            }
            let arguments = if call.arguments.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&call.arguments).map_err(|err| {
                    AgentError::Provider(ProviderError::InvalidResponse(format!(
                        "invalid streamed tool arguments: {err}"
                    )))
                })?
            };
            yield ProviderEvent::ToolCall(ToolCall {
                id: call.id,
                name: call.name,
                arguments,
            });
        }

        yield ProviderEvent::MessageStop { stop_reason, usage, model };
    }
}

async fn send_stream_json(
    client: &reqwest::Client,
    api_key: &str,
    url: &str,
    body: Value,
) -> Result<reqwest::Response, AgentError> {
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .header("User-Agent", concat!("telos_agent/", env!("CARGO_PKG_VERSION")))
        .json(&body)
        .send()
        .await
        .map_err(classify_reqwest_error)?;

    if response.status().is_success() {
        Ok(response)
    } else {
        Err(map_deepseek_http_error(response).await)
    }
}

#[derive(Debug, Default)]
struct StreamingToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn apply_stream_chunk(
    value: Value,
    tool_calls: &mut Vec<StreamingToolCall>,
    stop_reason: &mut StopReason,
    usage: &mut Option<TokenUsage>,
    model: &mut Option<String>,
) -> Result<Vec<ProviderEvent>, AgentError> {
    if let Some(chunk_usage) = value.get("usage").filter(|usage| !usage.is_null()) {
        *usage = Some(parse_usage(chunk_usage)?);
    }
    if let Some(chunk_model) = value.get("model").and_then(Value::as_str) {
        *model = Some(chunk_model.to_string());
    }

    let mut events = Vec::new();
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return Ok(events);
    };

    for choice in choices {
        if let Some(delta) = choice.get("delta") {
            if let Some(content) =
                delta.get("content").and_then(Value::as_str).filter(|text| !text.is_empty())
            {
                events.push(ProviderEvent::TextDelta(content.to_string()));
            }
            if let Some(reasoning) = delta
                .get("reasoning_content")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                events.push(ProviderEvent::ThinkingDelta(reasoning.to_string()));
            }
            if let Some(delta_tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for delta_call in delta_tool_calls {
                    let index =
                        delta_call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    while tool_calls.len() <= index {
                        tool_calls.push(StreamingToolCall::default());
                    }
                    let aggregate = &mut tool_calls[index];
                    if let Some(id) = delta_call.get("id").and_then(Value::as_str) {
                        aggregate.id = id.to_string();
                    }
                    if let Some(function) = delta_call.get("function") {
                        if let Some(name) = function.get("name").and_then(Value::as_str) {
                            aggregate.name = name.to_string();
                        }
                        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                            aggregate.arguments.push_str(arguments);
                        }
                    }
                }
            }
        }

        if choice.get("finish_reason").is_some() {
            *stop_reason = parse_stop_reason(choice.get("finish_reason"));
        }
    }

    Ok(events)
}
