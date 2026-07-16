use crate::error::AgentError;

/// Configuration for [`DeepSeekProvider`](crate::DeepSeekProvider).
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
    /// Build a config from an explicit API key and model.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.deepseek.com".into(),
        }
    }

    /// Build a config from `DEEPSEEK_API_KEY` and the given model.
    ///
    /// Reads the API key directly from the process environment. Callers that
    /// want to load a `.env` file should do so explicitly before calling this
    /// constructor.
    pub fn from_env(model: impl Into<String>) -> Result<Self, AgentError> {
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .map_err(|_| AgentError::Config("missing DEEPSEEK_API_KEY".into()))?;

        Ok(Self::new(api_key, model))
    }
}
