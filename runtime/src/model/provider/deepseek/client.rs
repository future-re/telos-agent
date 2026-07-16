use async_trait::async_trait;
use futures_core::stream::Stream;
use serde_json::Value;

use crate::error::{AgentError, ProviderError};
use crate::model::provider::{CompletionRequest, CompletionResponse, ModelProvider, ProviderEvent};

use super::config::DeepSeekConfig;
use super::error::{classify_reqwest_error, map_deepseek_http_error};
use super::request::{build_chat_request, build_fim_request};
use super::response::{parse_chat_response, parse_fim_response};
use super::stream::stream_json_to;
use super::types::{
    DeepSeekBalance, DeepSeekChatOptions, DeepSeekFimRequest, DeepSeekFimResponse,
    DeepSeekModelList,
};
use super::url::{normalize_beta_url, normalize_chat_url, normalize_v1_url};

/// [`ModelProvider`] implementation backed by the DeepSeek API.
pub struct DeepSeekProvider {
    pub(super) client: reqwest::Client,
    pub(super) api_key: String,
    pub(super) model: String,
    pub(super) chat_url: String,
    pub(super) beta_chat_url: String,
    pub(super) fim_url: String,
    pub(super) models_url: String,
    pub(super) balance_url: String,
}

impl DeepSeekProvider {
    pub fn new(config: DeepSeekConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: config.api_key,
            model: config.model,
            chat_url: normalize_chat_url(&config.base_url),
            beta_chat_url: normalize_beta_url(&config.base_url, "chat/completions"),
            fim_url: normalize_beta_url(&config.base_url, "completions"),
            models_url: normalize_v1_url(&config.base_url, "models"),
            balance_url: normalize_v1_url(&config.base_url, "user/balance"),
        }
    }

    pub async fn complete_with_options(
        &self,
        request: CompletionRequest,
        options: DeepSeekChatOptions,
    ) -> Result<CompletionResponse, AgentError> {
        let body = build_chat_request(&self.model, &request, false, &options);
        let response = self.send_json_to(self.chat_url_for_options(&options), body).await?;
        let value: Value = response
            .json()
            .await
            .map_err(|err| AgentError::Provider(ProviderError::InvalidResponse(err.to_string())))?;
        parse_chat_response(value)
    }

    pub fn stream_complete_with_options<'a>(
        &'a self,
        request: CompletionRequest,
        options: DeepSeekChatOptions,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        let body = build_chat_request(&self.model, &request, true, &options);
        let url = self.chat_url_for_options(&options).to_string();
        Box::pin(stream_json_to(&self.client, &self.api_key, url, body))
    }

    pub async fn fim_complete(
        &self,
        request: DeepSeekFimRequest,
    ) -> Result<DeepSeekFimResponse, AgentError> {
        let body = build_fim_request(&self.model, request);
        let response = self.send_json_to(&self.fim_url, body).await?;
        let value: Value = response
            .json()
            .await
            .map_err(|err| AgentError::Provider(ProviderError::InvalidResponse(err.to_string())))?;
        parse_fim_response(value)
    }

    pub async fn list_models(&self) -> Result<DeepSeekModelList, AgentError> {
        self.send_get_json(&self.models_url).await
    }

    pub async fn balance(&self) -> Result<DeepSeekBalance, AgentError> {
        self.send_get_json(&self.balance_url).await
    }

    async fn send_json(&self, body: Value) -> Result<reqwest::Response, AgentError> {
        self.send_json_to(&self.chat_url, body).await
    }

    async fn send_json_to(&self, url: &str, body: Value) -> Result<reqwest::Response, AgentError> {
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
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

    async fn send_get_json<T>(&self, url: &str) -> Result<T, AgentError>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.api_key)
            .header("User-Agent", concat!("telos_agent/", env!("CARGO_PKG_VERSION")))
            .send()
            .await
            .map_err(classify_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_deepseek_http_error(response).await);
        }

        response
            .json()
            .await
            .map_err(|err| AgentError::Provider(ProviderError::InvalidResponse(err.to_string())))
    }

    fn chat_url_for_options(&self, options: &DeepSeekChatOptions) -> &str {
        if options.prefix.is_some() { &self.beta_chat_url } else { &self.chat_url }
    }
}

#[async_trait]
impl ModelProvider for DeepSeekProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError> {
        let body =
            build_chat_request(&self.model, &request, false, &DeepSeekChatOptions::default());
        let response = self.send_json(body).await?;
        let value: Value = response
            .json()
            .await
            .map_err(|err| AgentError::Provider(ProviderError::InvalidResponse(err.to_string())))?;
        parse_chat_response(value)
    }

    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        let body = build_chat_request(&self.model, &request, true, &DeepSeekChatOptions::default());
        Box::pin(stream_json_to(&self.client, &self.api_key, self.chat_url.clone(), body))
    }
}
