use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use telos_agent::{
    AgentConfig, ApprovalHandler, DeepSeekConfig, DeepSeekProvider, KimiConfig, KimiProvider,
    MockProvider,
};

use crate::cli::{ProviderArg, SharedOptions};

/// Configuration loaded from toml files (user-level ~/.config/telos/config.toml
/// and project-level .telos.toml).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FileConfig {
    pub agent: Option<AgentSection>,
    pub display: Option<DisplaySection>,
    pub approval: Option<ApprovalSection>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentSection {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub max_iterations: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DisplaySection {
    pub theme: Option<String>,
    pub render_markdown: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalSection {
    pub default_policy: Option<String>,
    pub policies: Option<HashMap<String, String>>,
}

/// Load config from an explicit file path. Returns `Ok(None)` if the file
/// does not exist.
pub fn load_config_file(path: &Path) -> Result<Option<FileConfig>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;
    let cfg: FileConfig = toml::from_str(&contents)
        .with_context(|| format!("failed to parse config file: {}", path.display()))?;
    Ok(Some(cfg))
}

/// Load user config from the given path, or from the default
/// `~/.config/telos/config.toml` if `config_path` is `None`.
pub fn load_user_config(config_path: Option<&Path>) -> Result<Option<FileConfig>> {
    let path = match config_path {
        Some(p) => p.to_path_buf(),
        None => {
            let base = dirs::config_dir().context("could not determine user config directory")?;
            base.join("telos").join("config.toml")
        }
    };
    load_config_file(&path)
}

/// Load config from a project `.telos.toml` located under `dir`.
pub fn load_project_config(dir: &Path) -> Result<Option<FileConfig>> {
    let path = dir.join(".telos.toml");
    load_config_file(&path)
}

/// Merge two config layers. `project` values override `user` values.
/// Fields set to `None` on the project layer fall through to the user layer.
pub fn merge_configs(user: Option<FileConfig>, project: Option<FileConfig>) -> FileConfig {
    let agent = merge_agent(
        user.as_ref().and_then(|c| c.agent.as_ref()),
        project.as_ref().and_then(|c| c.agent.as_ref()),
    );
    let display = merge_display(
        user.as_ref().and_then(|c| c.display.as_ref()),
        project.as_ref().and_then(|c| c.display.as_ref()),
    );
    let approval = merge_approval(
        user.as_ref().and_then(|c| c.approval.as_ref()),
        project.as_ref().and_then(|c| c.approval.as_ref()),
    );
    let env = match (user.and_then(|c| c.env), project.and_then(|c| c.env)) {
        (Some(mut u), Some(p)) => {
            u.extend(p);
            Some(u)
        }
        (Some(u), None) => Some(u),
        (None, Some(p)) => Some(p),
        (None, None) => None,
    };

    FileConfig { agent, display, approval, env }
}

fn merge_agent(
    user: Option<&AgentSection>,
    project: Option<&AgentSection>,
) -> Option<AgentSection> {
    match (user, project) {
        (None, None) => None,
        (Some(u), None) => Some(AgentSection {
            model: u.model.clone(),
            provider: u.provider.clone(),
            max_iterations: u.max_iterations,
        }),
        (None, Some(p)) => Some(AgentSection {
            model: p.model.clone(),
            provider: p.provider.clone(),
            max_iterations: p.max_iterations,
        }),
        (Some(u), Some(p)) => Some(AgentSection {
            model: p.model.clone().or_else(|| u.model.clone()),
            provider: p.provider.clone().or_else(|| u.provider.clone()),
            max_iterations: p.max_iterations.or(u.max_iterations),
        }),
    }
}

fn merge_display(
    user: Option<&DisplaySection>,
    project: Option<&DisplaySection>,
) -> Option<DisplaySection> {
    match (user, project) {
        (None, None) => None,
        (Some(u), None) => {
            Some(DisplaySection { theme: u.theme.clone(), render_markdown: u.render_markdown })
        }
        (None, Some(p)) => {
            Some(DisplaySection { theme: p.theme.clone(), render_markdown: p.render_markdown })
        }
        (Some(u), Some(p)) => Some(DisplaySection {
            theme: p.theme.clone().or_else(|| u.theme.clone()),
            render_markdown: p.render_markdown.or(u.render_markdown),
        }),
    }
}

fn merge_approval(
    user: Option<&ApprovalSection>,
    project: Option<&ApprovalSection>,
) -> Option<ApprovalSection> {
    match (user, project) {
        (None, None) => None,
        (Some(u), None) => Some(ApprovalSection {
            default_policy: u.default_policy.clone(),
            policies: u.policies.clone(),
        }),
        (None, Some(p)) => Some(ApprovalSection {
            default_policy: p.default_policy.clone(),
            policies: p.policies.clone(),
        }),
        (Some(u), Some(p)) => Some(ApprovalSection {
            default_policy: p.default_policy.clone().or_else(|| u.default_policy.clone()),
            policies: p.policies.clone().or_else(|| u.policies.clone()),
        }),
    }
}

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
