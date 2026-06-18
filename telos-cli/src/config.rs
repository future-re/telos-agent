use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use telos_agent::{AgentConfig, KimiConfig, KimiProvider, MockProvider};

use crate::cli::{ProviderArg, SharedOptions};

pub enum ResolvedProvider {
    Kimi(KimiProvider),
    Mock(MockProvider),
}

pub fn build_agent_config(options: &SharedOptions) -> Result<AgentConfig> {
    let mut config = AgentConfig::default();

    if let Some(cwd) = &options.cwd {
        config.cwd = cwd.clone();
    }

    config.max_iterations = options.max_iterations;
    config.auto_validate_schema = !options.no_validate_schema;

    // Inherit a safe subset of the process environment (PATH, HOME).
    let mut env = HashMap::new();
    for key in ["PATH", "HOME"] {
        if let Ok(value) = std::env::var(key) {
            env.insert(key.to_string(), value);
        }
    }
    config.env = env;

    Ok(config)
}

pub fn build_provider(options: &SharedOptions) -> Result<ResolvedProvider> {
    let provider = options.provider.unwrap_or(ProviderArg::Mock);

    match provider {
        ProviderArg::Kimi => {
            let model = options.model.clone().unwrap_or_else(|| "kimi-k2-0711-preview".into());
            let cfg = KimiConfig::from_env(model)
                .context("failed to build Kimi config; set KIMI_API_KEY")?;
            Ok(ResolvedProvider::Kimi(KimiProvider::new(cfg)))
        }
        ProviderArg::Deepseek => {
            anyhow::bail!("DeepSeek provider is not yet wired in telos-cli")
        }
        ProviderArg::Mock => Ok(ResolvedProvider::Mock(MockProvider::new(vec![]))),
    }
}

pub fn default_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
