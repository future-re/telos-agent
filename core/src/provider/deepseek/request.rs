use serde_json::{Value, json};

use crate::message::{Message, Role, ToolCall, ToolResult};
use crate::provider::{CompletionRequest, ModelHint};
use crate::tool::ToolDefinition;

use super::types::{DeepSeekChatOptions, DeepSeekFimRequest, DeepSeekResponseFormat};

pub(super) fn build_chat_request(
    model: &str,
    request: &CompletionRequest,
    stream: bool,
    options: &DeepSeekChatOptions,
) -> Value {
    let mut messages: Vec<Value> = request.messages.iter().flat_map(message_to_deepseek).collect();

    if let Some(system_prompt) = request.system_prompt_text() {
        let already_has_system = matches!(
            messages.first().and_then(|m| m.get("role")).and_then(Value::as_str),
            Some("system")
        );
        if !already_has_system {
            messages.insert(0, json!({ "role": "system", "content": system_prompt }));
        }
    }

    let mut body = serde_json::Map::new();
    body.insert("model".into(), Value::String(model.to_string()));
    body.insert("messages".into(), Value::Array(messages));

    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(request.tools.iter().map(tool_to_deepseek).collect()),
        );
    }

    if let Some(max_tokens) = request.max_tokens {
        body.insert("max_tokens".into(), json!(max_tokens));
    }

    if matches!(request.model_hint, Some(ModelHint::Thinking | ModelHint::Recovery)) {
        let budget_tokens = request.max_tokens.map(|m| m.saturating_sub(1)).unwrap_or(127_999);
        body.insert(
            "thinking".into(),
            json!({ "type": "enabled", "budget_tokens": budget_tokens }),
        );
    }

    if let Some(format) = options.response_format {
        body.insert("response_format".into(), response_format_to_deepseek(format));
    }

    if let Some(prefix) = &options.prefix {
        messages_push_prefix(&mut body, prefix);
    }

    if let Some(stop) = &options.stop {
        body.insert("stop".into(), Value::Array(stop.iter().cloned().map(Value::String).collect()));
    }

    if stream {
        body.insert("stream".into(), Value::Bool(true));
        body.insert("stream_options".into(), json!({ "include_usage": true }));
    }

    Value::Object(body)
}

pub(super) fn build_fim_request(model: &str, request: DeepSeekFimRequest) -> Value {
    let mut body = serde_json::Map::new();
    body.insert("model".into(), Value::String(request.model.unwrap_or_else(|| model.to_string())));
    body.insert("prompt".into(), Value::String(request.prompt));
    if let Some(suffix) = request.suffix {
        body.insert("suffix".into(), Value::String(suffix));
    }
    if let Some(max_tokens) = request.max_tokens {
        body.insert("max_tokens".into(), json!(max_tokens));
    }
    if let Some(temperature) = request.temperature {
        body.insert("temperature".into(), json!(temperature));
    }
    if let Some(top_p) = request.top_p {
        body.insert("top_p".into(), json!(top_p));
    }
    if let Some(stop) = request.stop {
        body.insert("stop".into(), Value::Array(stop.into_iter().map(Value::String).collect()));
    }
    Value::Object(body)
}

fn response_format_to_deepseek(format: DeepSeekResponseFormat) -> Value {
    match format {
        DeepSeekResponseFormat::JsonObject => json!({ "type": "json_object" }),
    }
}

fn messages_push_prefix(body: &mut serde_json::Map<String, Value>, prefix: &str) {
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        messages.push(json!({
            "role": "assistant",
            "content": prefix,
            "prefix": true,
        }));
    }
}

fn message_to_deepseek(message: &Message) -> Vec<Value> {
    match message.role {
        Role::System => vec![json!({ "role": "system", "content": message.text_content() })],
        Role::User => vec![json!({ "role": "user", "content": message.text_content() })],
        Role::Assistant => {
            let mut body = serde_json::Map::new();
            body.insert("role".into(), Value::String("assistant".into()));
            let text = message.text_content();
            if !text.is_empty() {
                body.insert("content".into(), Value::String(text));
            }
            let tool_calls = message.tool_calls().map(tool_call_to_deepseek).collect::<Vec<_>>();
            if !tool_calls.is_empty() {
                body.insert("tool_calls".into(), Value::Array(tool_calls));
            }
            vec![Value::Object(body)]
        }
        Role::Tool => message.tool_results_iter().map(tool_result_to_deepseek).collect(),
    }
}

fn tool_call_to_deepseek(call: &ToolCall) -> Value {
    json!({
        "id": call.id,
        "type": "function",
        "function": {
            "name": call.name,
            "arguments": call.arguments.to_string(),
        }
    })
}

fn tool_result_to_deepseek(result: &ToolResult) -> Value {
    json!({
        "role": "tool",
        "tool_call_id": result.tool_call_id,
        "content": result.content.to_string(),
    })
}

fn tool_to_deepseek(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    })
}
