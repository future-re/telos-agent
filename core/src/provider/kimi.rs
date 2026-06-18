//! Kimi (Moonshot AI) API provider.
//!
//! Kimi's API is OpenAI-compatible, so this provider uses [`async_openai`] with
//! Kimi's base URL and API-key convention.

use async_openai::Client;
use async_openai::config::OpenAIConfig as AsyncOpenAIConfig;
use async_trait::async_trait;
use futures_core::stream::Stream;

use crate::error::AgentError;
use crate::provider::{CompletionRequest, CompletionResponse, ModelProvider, ProviderEvent};

/// Configuration for [`KimiProvider`].
#[derive(Clone)]
pub struct KimiConfig {
    pub api_key: String,
    pub model: String,
    /// Base URL — override to talk to a Kimi-compatible service.
    /// The provider automatically appends `/v1` if it is not present.
    pub base_url: String,
}

impl std::fmt::Debug for KimiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KimiConfig")
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl KimiConfig {
    /// Build a config from an explicit API key and model.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.moonshot.cn".into(),
        }
    }

    /// Build a config from `MOONSHOT_API_KEY` and the given model.
    ///
    /// Reads the API key directly from the process environment. Callers that
    /// want to load a `.env` file should do so explicitly before calling this
    /// constructor.
    pub fn from_env(model: impl Into<String>) -> Result<Self, AgentError> {
        let api_key = std::env::var("MOONSHOT_API_KEY")
            .map_err(|_| AgentError::Config("missing MOONSHOT_API_KEY".into()))?;

        Ok(Self::new(api_key, model))
    }
}

/// [`ModelProvider`] implementation backed by the Kimi (Moonshot AI) API.
pub struct KimiProvider {
    client: Client<AsyncOpenAIConfig>,
    model: String,
}

impl KimiProvider {
    pub fn new(config: KimiConfig) -> Self {
        Self {
            client: crate::provider::openai_compat::build_client(&config.api_key, &config.base_url),
            model: config.model,
        }
    }
}

#[async_trait]
impl ModelProvider for KimiProvider {
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

    fn test_config(base_url: String) -> KimiConfig {
        KimiConfig { api_key: "test-moonshot-key".into(), model: "kimi-k2.7-code".into(), base_url }
    }

    #[test]
    fn default_base_url() {
        let config = KimiConfig {
            api_key: "x".into(),
            model: "kimi-k2.7-code".into(),
            base_url: "https://api.moonshot.cn".into(),
        };
        let provider = KimiProvider::new(config);
        // Provider stores the model and builds a client internally.
        assert_eq!(provider.model, "kimi-k2.7-code");
    }

    #[tokio::test]
    async fn completes_chat_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer test-moonshot-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1,
                "model": "kimi-k2.7-code",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "Hello from Kimi!" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 8, "completion_tokens": 4, "total_tokens": 12 }
            })))
            .mount(&server)
            .await;

        let provider = KimiProvider::new(test_config(server.uri()));
        let request = CompletionRequest {
            system_prompt: None,
            system_prompt_blocks: None,
            messages: vec![Message::user("Hi")],
            tools: vec![],
        };

        let response = provider.complete(request).await.unwrap();
        assert_eq!(response.message.text_content(), "Hello from Kimi!");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.usage, Some(TokenUsage { input_tokens: 8, output_tokens: 4 }));
    }

    #[tokio::test]
    async fn streams_chat_response() {
        let server = MockServer::start().await;
        let body = "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"kimi-k2.7-code\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n\
            data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"kimi-k2.7-code\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" there\"},\"finish_reason\":null}]}\n\n\
            data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"kimi-k2.7-code\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
            data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer test-moonshot-key"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .mount(&server)
            .await;

        let provider = KimiProvider::new(test_config(server.uri()));
        let request = CompletionRequest {
            system_prompt: None,
            system_prompt_blocks: None,
            messages: vec![Message::user("Hello")],
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
        assert_eq!(text, "Hi there");
    }
}
