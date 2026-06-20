//! Hint-based model routing provider.
//!
//! [`RoutedModelConfig`] maps [`ModelHint`] values to concrete model names.
//! [`RoutedProvider`] implements [`ModelProvider`] by resolving the hint on
//! each request and delegating to a pre-created [`DeepSeekProvider`].

use std::collections::{HashMap, HashSet};
use std::pin::Pin;

use async_trait::async_trait;
use futures_core::stream::Stream;

use crate::error::AgentError;
use crate::provider::ModelProvider;
use crate::provider::deepseek::{DeepSeekConfig, DeepSeekProvider};
use crate::provider::types::{CompletionRequest, CompletionResponse, ModelHint, ProviderEvent};

/// Maps [`ModelHint`] values to concrete model names.
///
/// Hints not present in the map fall back to `default_model`.
#[derive(Clone)]
pub struct RoutedModelConfig {
    /// hint → model_name mapping
    pub routes: HashMap<ModelHint, String>,
    /// Model used when no hint matches or hint is `None`
    pub default_model: String,
    /// API key shared across all routed models
    pub api_key: String,
    /// Base URL shared across all routed models
    pub base_url: String,
}

impl std::fmt::Debug for RoutedModelConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutedModelConfig")
            .field("routes", &self.routes)
            .field("default_model", &self.default_model)
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl RoutedModelConfig {
    /// Resolve a hint to a concrete model name.
    /// Returns `default_model` when hint is `None` or not in the routes map.
    pub fn resolve(&self, hint: Option<ModelHint>) -> &str {
        hint.and_then(|h| self.routes.get(&h).map(|s| s.as_str())).unwrap_or(&self.default_model)
    }

    /// Convenience constructor for the common two-model case.
    ///
    /// Routes Thinking + Recovery → `thinking`, Execution + Summarization → `execution`.
    /// Default model = `execution` (fast path).
    pub fn dual(api_key: String, thinking: String, execution: String) -> Self {
        let mut routes = HashMap::new();
        routes.insert(ModelHint::Thinking, thinking.clone());
        routes.insert(ModelHint::Recovery, thinking);
        routes.insert(ModelHint::Execution, execution.clone());
        routes.insert(ModelHint::Summarization, execution.clone());
        Self {
            routes,
            default_model: execution,
            api_key,
            base_url: "https://api.deepseek.com".into(),
        }
    }

    /// Set a custom base URL (e.g. for self-hosted or proxy endpoints).
    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    /// Collect all unique model names referenced in this config.
    fn all_models(&self) -> HashSet<&str> {
        let mut models: HashSet<&str> = self.routes.values().map(|s| s.as_str()).collect();
        models.insert(&self.default_model);
        models
    }
}

/// A [`ModelProvider`] that routes requests to different models based on
/// [`ModelHint`].
///
/// Providers are pre-created at construction time — one per unique model name
/// in the config. Provider selection is a simple HashMap lookup with no
/// allocation on the hot path.
pub struct RoutedProvider {
    config: RoutedModelConfig,
    /// model_name → provider (pre-created)
    providers: HashMap<String, DeepSeekProvider>,
}

impl RoutedProvider {
    pub fn new(config: RoutedModelConfig) -> Self {
        let mut providers = HashMap::new();
        for model in config.all_models() {
            let provider_config = DeepSeekConfig {
                api_key: config.api_key.clone(),
                model: model.to_string(),
                base_url: config.base_url.clone(),
            };
            providers.insert(model.to_string(), DeepSeekProvider::new(provider_config));
        }
        Self { config, providers }
    }

    /// Look up the provider for a given hint.
    fn resolve(&self, hint: Option<ModelHint>) -> &DeepSeekProvider {
        let model = self.config.resolve(hint);
        tracing::debug!(
            hint = ?hint,
            model = %model,
            "model route"
        );
        // Safety: new() pre-creates providers for every model in config
        &self.providers[model]
    }
}

#[async_trait]
impl ModelProvider for RoutedProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError> {
        let hint = request.model_hint;
        let provider = self.resolve(hint);
        let result = provider.complete(request).await;
        if let Ok(ref resp) = result {
            tracing::debug!(
                hint = ?hint,
                input_tokens = resp.usage.map(|u| u.input_tokens).unwrap_or(0),
                output_tokens = resp.usage.map(|u| u.output_tokens).unwrap_or(0),
                "routed complete"
            );
        }
        result
    }

    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        let hint = request.model_hint;
        let provider = self.resolve(hint);
        // provider borrows from self.providers (lifetime = 'a) ✅
        provider.stream_complete(request)
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        self.resolve(None).estimate_tokens(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_config() -> RoutedModelConfig {
        let mut routes = HashMap::new();
        routes.insert(ModelHint::Thinking, "deepseek-v4-pro".into());
        routes.insert(ModelHint::Execution, "deepseek-v4-flash".into());
        routes.insert(ModelHint::Recovery, "deepseek-v4-pro".into());
        routes.insert(ModelHint::Summarization, "deepseek-v4-flash".into());
        RoutedModelConfig {
            routes,
            default_model: "deepseek-v4-flash".into(),
            api_key: "test-key".into(),
            base_url: "https://api.deepseek.com".into(),
        }
    }

    #[test]
    fn resolve_known_hint_returns_correct_model() {
        let config = test_config();
        assert_eq!(config.resolve(Some(ModelHint::Thinking)), "deepseek-v4-pro");
        assert_eq!(config.resolve(Some(ModelHint::Execution)), "deepseek-v4-flash");
        assert_eq!(config.resolve(Some(ModelHint::Recovery)), "deepseek-v4-pro");
    }

    #[test]
    fn resolve_none_returns_default() {
        let config = test_config();
        assert_eq!(config.resolve(None), "deepseek-v4-flash");
    }

    #[test]
    fn dual_constructor_maps_correctly() {
        let config =
            RoutedModelConfig::dual("key".into(), "pro-model".into(), "flash-model".into());
        assert_eq!(config.resolve(Some(ModelHint::Thinking)), "pro-model");
        assert_eq!(config.resolve(Some(ModelHint::Recovery)), "pro-model");
        assert_eq!(config.resolve(Some(ModelHint::Execution)), "flash-model");
        assert_eq!(config.resolve(Some(ModelHint::Summarization)), "flash-model");
        assert_eq!(config.resolve(None), "flash-model");
    }

    #[test]
    fn all_models_collects_unique_names() {
        let config = test_config();
        let models = config.all_models();
        assert_eq!(models.len(), 2);
        assert!(models.contains("deepseek-v4-pro"));
        assert!(models.contains("deepseek-v4-flash"));
    }

    #[test]
    fn routed_provider_constructs_without_error() {
        let config = test_config();
        let provider = RoutedProvider::new(config);
        // Just verify construction succeeds and providers map is populated
        assert_eq!(provider.providers.len(), 2);
    }

    #[test]
    fn estimate_tokens_delegates_to_default() {
        let config = test_config();
        let provider = RoutedProvider::new(config);
        // estimate_tokens uses the default provider; actual value depends on
        // tiktoken-rs but should be > 0 for non-empty text
        let tokens = provider.estimate_tokens("hello world");
        assert!(tokens > 0);
    }
}
