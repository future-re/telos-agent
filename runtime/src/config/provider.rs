use std::collections::HashMap;
use std::io::IsTerminal;
use std::sync::Arc;

use anyhow::{Context, Result};
use telos_agent::{
    AgentConfig, ApprovalHandler, DeepSeekConfig, DeepSeekProvider, MockProvider,
    RoutedModelConfig, RoutedProvider,
};

use super::FileConfig;
use crate::options::{ProviderKind, ProviderSetup, SharedOptions};

const DEEPSEEK_AUTO_MODE: &str = "hybrid";
const DEEPSEEK_AUTO_MODE_OLD: &str = "auto";
const DEEPSEEK_PRO_ALIAS: &str = "pro";
const DEEPSEEK_FLASH_ALIAS: &str = "flash";
const DEEPSEEK_PRO_MODEL: &str = "deepseek-v4-pro";
const DEEPSEEK_FLASH_MODEL: &str = "deepseek-v4-flash";

pub enum ResolvedProvider {
    DeepSeek(DeepSeekProvider),
    Routed(RoutedProvider),
    Mock(MockProvider),
}

enum DeepSeekModelSelection {
    Routed { thinking: String, fast: String },
    Single(String),
}

pub fn build_agent_config(
    options: &SharedOptions,
    config: &FileConfig,
    approval_handler: Option<Arc<dyn ApprovalHandler>>,
) -> Result<AgentConfig> {
    let mut agent_config = AgentConfig::default();

    if let Some(cwd) = &options.cwd {
        agent_config.cwd = cwd.clone();
    }

    agent_config.max_iterations =
        options.max_iterations.or_else(|| config.agent.as_ref()?.max_iterations);
    agent_config.auto_validate_schema = !options.no_validate_schema;
    agent_config.approval_handler = approval_handler;

    let mut env = telos_agent::platform_base_env();
    if let Some(config_env) = &config.env {
        for (key, value) in config_env {
            env.insert(key.clone(), value.clone());
        }
    }
    agent_config.env = env;

    Ok(agent_config)
}

pub fn build_provider(options: &SharedOptions, config: &FileConfig) -> Result<ResolvedProvider> {
    let provider =
        options.provider.or_else(|| provider_from_config(config)).unwrap_or(ProviderKind::Mock);
    let config_env = config.env.as_ref();

    match provider {
        ProviderKind::Deepseek => {
            let api_key =
                resolve_api_key(provider, options.api_key.clone(), config_env, "DEEPSEEK_API_KEY")?;

            match resolve_deepseek_model_selection(options, config) {
                DeepSeekModelSelection::Routed { thinking, fast } if thinking != fast => {
                    let routed_config = RoutedModelConfig::dual(api_key, thinking, fast);
                    Ok(ResolvedProvider::Routed(RoutedProvider::new(routed_config)))
                }
                DeepSeekModelSelection::Routed { thinking, .. } => {
                    let cfg = DeepSeekConfig::new(api_key, thinking);
                    Ok(ResolvedProvider::DeepSeek(DeepSeekProvider::new(cfg)))
                }
                DeepSeekModelSelection::Single(model) => {
                    let cfg = DeepSeekConfig::new(api_key, model);
                    Ok(ResolvedProvider::DeepSeek(DeepSeekProvider::new(cfg)))
                }
            }
        }
        ProviderKind::Mock => Ok(ResolvedProvider::Mock(MockProvider::new(vec![]))),
    }
}

pub fn build_provider_from_setup(result: &ProviderSetup) -> Result<ResolvedProvider> {
    match result.provider {
        ProviderKind::Deepseek => {
            if result.thinking_model != result.fast_model {
                let routed_config = RoutedModelConfig::dual(
                    result.api_key.clone(),
                    result.thinking_model.clone(),
                    result.fast_model.clone(),
                );
                Ok(ResolvedProvider::Routed(RoutedProvider::new(routed_config)))
            } else {
                let cfg = DeepSeekConfig::new(&result.api_key, &result.thinking_model);
                Ok(ResolvedProvider::DeepSeek(DeepSeekProvider::new(cfg)))
            }
        }
        ProviderKind::Mock => Ok(ResolvedProvider::Mock(MockProvider::new(vec![]))),
    }
}

fn resolve_deepseek_model_selection(
    options: &SharedOptions,
    config: &FileConfig,
) -> DeepSeekModelSelection {
    let explicit_model = options.model.clone().or_else(|| config.agent.as_ref()?.model.clone());

    match explicit_model.as_deref().map(str::trim).filter(|model| !model.is_empty()) {
        Some(model)
            if model.eq_ignore_ascii_case(DEEPSEEK_AUTO_MODE)
                || model.eq_ignore_ascii_case(DEEPSEEK_AUTO_MODE_OLD) =>
        {
            DeepSeekModelSelection::Routed {
                thinking: configured_thinking_model(options, config),
                fast: configured_fast_model(options, config),
            }
        }
        Some(model) if model.eq_ignore_ascii_case(DEEPSEEK_PRO_ALIAS) => {
            DeepSeekModelSelection::Single(DEEPSEEK_PRO_MODEL.into())
        }
        Some(model) if model.eq_ignore_ascii_case(DEEPSEEK_FLASH_ALIAS) => {
            DeepSeekModelSelection::Single(DEEPSEEK_FLASH_MODEL.into())
        }
        Some(model) => DeepSeekModelSelection::Single(model.to_string()),
        None => DeepSeekModelSelection::Routed {
            thinking: configured_thinking_model(options, config),
            fast: configured_fast_model(options, config),
        },
    }
}

fn configured_thinking_model(options: &SharedOptions, config: &FileConfig) -> String {
    options
        .thinking_model
        .clone()
        .or_else(|| config.agent.as_ref()?.models.as_ref()?.thinking.clone())
        .unwrap_or_else(|| DEEPSEEK_PRO_MODEL.into())
}

fn configured_fast_model(options: &SharedOptions, config: &FileConfig) -> String {
    options
        .fast_model
        .clone()
        .or_else(|| config.agent.as_ref()?.models.as_ref()?.fast.clone())
        .unwrap_or_else(|| DEEPSEEK_FLASH_MODEL.into())
}

fn provider_from_config(config: &FileConfig) -> Option<ProviderKind> {
    let provider = config.agent.as_ref()?.provider.as_deref()?;
    match provider.to_lowercase().as_str() {
        "deepseek" | "deep" => Some(ProviderKind::Deepseek),
        "mock" => Some(ProviderKind::Mock),
        _ => None,
    }
}

#[deprecated(
    since = "0.2.0",
    note = "config env vars are now read directly from FileConfig::env; apply_config_env is a no-op and will be removed"
)]
pub fn apply_config_env(_config: &FileConfig) {}

fn resolve_api_key(
    provider: ProviderKind,
    cli_key: Option<String>,
    config_env: Option<&HashMap<String, String>>,
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

    if let Some(key) = config_env.and_then(|env| env.get(env_var)).map(String::as_str)
        && !key.trim().is_empty()
    {
        return Ok(key.to_string());
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

fn provider_name(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Deepseek => "DeepSeek",
        ProviderKind::Mock => "Mock",
    }
}
