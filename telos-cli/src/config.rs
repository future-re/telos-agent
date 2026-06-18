use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;
use telos_agent::{
    AgentConfig, ApprovalHandler, DeepSeekConfig, DeepSeekProvider, KimiConfig, KimiProvider,
    MockProvider,
};

use crate::cli::{ProviderArg, SharedOptions};

pub enum ResolvedProvider {
    DeepSeek(DeepSeekProvider),
    Kimi(KimiProvider),
    Mock(MockProvider),
}

pub fn build_agent_config(
    options: &SharedOptions,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<AgentConfig> {
    let mut config = AgentConfig::default();

    if let Some(cwd) = &options.cwd {
        config.cwd = cwd.clone();
    }

    config.max_iterations = options.max_iterations;
    config.auto_validate_schema = !options.no_validate_schema;
    config.approval_handler = approval_handler;

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
            let api_key = resolve_api_key(provider, options.api_key.clone(), "MOONSHOT_API_KEY")?;
            let cfg = KimiConfig::new(api_key, model);
            Ok(ResolvedProvider::Kimi(KimiProvider::new(cfg)))
        }
        ProviderArg::Deepseek => {
            let model = options.model.clone().unwrap_or_else(|| "deepseek-chat".into());
            let api_key = resolve_api_key(provider, options.api_key.clone(), "DEEPSEEK_API_KEY")?;
            let cfg = DeepSeekConfig::new(api_key, model);
            Ok(ResolvedProvider::DeepSeek(DeepSeekProvider::new(cfg)))
        }
        ProviderArg::Mock => Ok(ResolvedProvider::Mock(MockProvider::new(vec![]))),
    }
}

fn resolve_api_key(
    provider: ProviderArg,
    cli_key: Option<String>,
    env_var: &str,
) -> Result<String> {
    if let Some(key) = cli_key {
        return Ok(key);
    }

    if let Ok(key) = std::env::var(env_var)
        && !key.trim().is_empty()
    {
        return Ok(key);
    }

    if std::io::stdin().is_terminal() {
        let name = provider_name(provider);
        eprintln!("Please enter your {name} API key (input will be hidden):");
        let key = rpassword::prompt_password("API key: ")
            .context("failed to read API key from terminal")?;
        if key.trim().is_empty() {
            anyhow::bail!("API key cannot be empty");
        }
        return Ok(key);
    }

    anyhow::bail!(
        "missing API key for {provider}; set {env_var} or pass --api-key",
        provider = provider_name(provider),
    )
}

fn provider_name(provider: ProviderArg) -> &'static str {
    match provider {
        ProviderArg::Kimi => "Kimi",
        ProviderArg::Deepseek => "DeepSeek",
        ProviderArg::Mock => "Mock",
    }
}

pub fn default_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
