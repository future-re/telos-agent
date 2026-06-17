//! DeepSeek API provider.
//!
//! DeepSeek's API is OpenAI-compatible, so this provider uses [`async_openai`]
//! with DeepSeek's base URL and API-key convention.

use async_openai::config::OpenAIConfig as AsyncOpenAIConfig;
use async_openai::Client;
use async_trait::async_trait;
use futures_core::stream::Stream;

use crate::error::AgentError;
use crate::provider::{CompletionRequest, CompletionResponse, ModelProvider, ProviderEvent};

/// Configuration for [`DeepSeekProvider`].
#[derive(Clone)]
pub struct DeepSeekConfig {
    pub api_key: String,
    pub model: String,
    /// Base URL — override to talk to a DeepSeek-compatible service.
    /// The provider automatically appends `/v1` if it is not present.
    pub base_url: String,
}

impl std::fmt::Debug for DeepSeekConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeepSeekConfig")
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl DeepSeekConfig {
    /// Build a config from `DEEPSEEK_API_KEY` and the given model.
    ///
    /// First loads a local `.env` file (if present) via [`dotenvy`], then reads
    /// the variable from the environment.
    pub fn from_env(model: impl Into<String>) -> Result<Self, AgentError> {
        dotenvy::dotenv().ok();
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .map_err(|_| AgentError::Config("missing DEEPSEEK_API_KEY".into()))?;

        Ok(Self {
            api_key,
            model: model.into(),
            base_url: "https://api.deepseek.com".into(),
        })
    }
}

/// [`ModelProvider`] implementation backed by the DeepSeek API.
pub struct DeepSeekProvider {
    client: Client<AsyncOpenAIConfig>,
    model: String,
}

impl DeepSeekProvider {
    pub fn new(config: DeepSeekConfig) -> Self {
        Self {
            client: crate::provider::openai_compat::build_client(&config.api_key, &config.base_url),
            model: config.model,
        }
    }
}

#[async_trait]
impl ModelProvider for DeepSeekProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError> {
        let openai_request = crate::provider::openai_compat::build_request(&self.model, request);
        let response = self
            .client
            .chat()
            .create(openai_request)
            .await
            .map_err(crate::provider::openai_compat::classify_openai_error)?;
        crate::provider::openai_compat::parse_response(response)
    }

    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        crate::provider::openai_compat::stream_complete(
            self.client.clone(),
            self.model.clone(),
            request,
        )
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        // Rough heuristic for cl100k_base-like tokenizers.
        (text.len() as f64 / 4.0).ceil() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::provider::{ProviderEvent, StopReason, TokenUsage};
    use futures_util::StreamExt;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(base_url: String) -> DeepSeekConfig {
        DeepSeekConfig {
            api_key: "test-deepseek-key".into(),
            model: "deepseek-chat".into(),
            base_url,
        }
    }

    #[test]
    fn default_base_url() {
        let config = DeepSeekConfig {
            api_key: "x".into(),
            model: "deepseek-chat".into(),
            base_url: "https://api.deepseek.com".into(),
        };
        let provider = DeepSeekProvider::new(config);
        assert_eq!(provider.model, "deepseek-chat");
    }

    #[tokio::test]
    async fn completes_chat_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer test-deepseek-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1,
                "model": "deepseek-chat",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "Hello from DeepSeek!" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 10, "completion_tokens": 3, "total_tokens": 13 }
            })))
            .mount(&server)
            .await;

        let provider = DeepSeekProvider::new(test_config(server.uri()));
        let request = CompletionRequest {
            system_prompt: None,
            messages: vec![Message::user("Hi")],
            tools: vec![],
        };

        let response = provider.complete(request).await.unwrap();
        assert_eq!(response.message.text_content(), "Hello from DeepSeek!");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(
            response.usage,
            Some(TokenUsage {
                input_tokens: 10,
                output_tokens: 3,
            })
        );
    }

    #[tokio::test]
    async fn streams_chat_response() {
        let server = MockServer::start().await;
        let body = "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n\
            data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"!\"},\"finish_reason\":null}]}\n\n\
            data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
            data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer test-deepseek-key"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .mount(&server)
            .await;

        let provider = DeepSeekProvider::new(test_config(server.uri()));
        let request = CompletionRequest {
            system_prompt: None,
            messages: vec![Message::user("Hi")],
            tools: vec![],
        };

        let events: Vec<_> = provider.stream_complete(request).collect().await;
        let mut text = String::new();
        let mut saw_start = false;
        let mut saw_stop = false;
        for event in events {
            match event.unwrap() {
                ProviderEvent::MessageStart => saw_start = true,
                ProviderEvent::TextDelta(delta) => text.push_str(&delta),
                ProviderEvent::MessageStop { stop_reason, .. } => {
                    saw_stop = true;
                    assert_eq!(stop_reason, StopReason::EndTurn);
                }
                _ => panic!("unexpected event"),
            }
        }
        assert!(saw_start);
        assert!(saw_stop);
        assert_eq!(text, "Hello!");
    }
}
