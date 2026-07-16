use serde_json::Value;

use crate::error::{AgentError, ProviderError};

pub(super) async fn map_deepseek_http_error(response: reqwest::Response) -> AgentError {
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    let provider_message = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| body.trim().to_string());
    let label = deepseek_error_label(status);
    let message = if provider_message.is_empty() {
        label.to_string()
    } else {
        format!("{label}: {provider_message}")
    };
    AgentError::Provider(ProviderError::Http { status, message })
}

fn deepseek_error_label(status: u16) -> &'static str {
    match status {
        400 => "DeepSeek bad request",
        401 => "DeepSeek authentication failed",
        402 => "DeepSeek insufficient balance",
        422 => "DeepSeek invalid parameters",
        429 => "DeepSeek rate limit reached",
        500 => "DeepSeek server error",
        503 => "DeepSeek server overloaded",
        _ => "DeepSeek HTTP error",
    }
}

pub(super) fn classify_reqwest_error(err: reqwest::Error) -> AgentError {
    if err.is_timeout() {
        AgentError::Provider(ProviderError::Timeout)
    } else if let Some(status) = err.status() {
        AgentError::Provider(ProviderError::Http {
            status: status.as_u16(),
            message: err.to_string(),
        })
    } else if err.is_decode() {
        AgentError::Provider(ProviderError::InvalidResponse(err.to_string()))
    } else {
        AgentError::Provider(ProviderError::Network(err.to_string()))
    }
}
