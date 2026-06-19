use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use telos_agent::{
    AgentConfig, ApprovalHandler, DeepSeekConfig, DeepSeekProvider, MockProvider,
    RoutedModelConfig, RoutedProvider,
};

use crate::cli::{ProviderArg, SharedOptions};
use crate::onboarding::OnboardingResult;

/// Configuration loaded from toml files (user-level ~/.config/telos/config.toml
/// and project-level .telos.toml).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FileConfig {
    pub agent: Option<AgentSection>,
    pub approval: Option<ApprovalSection>,
    pub codeql: Option<CodeqlConfigSection>,
    pub env: Option<HashMap<String, String>>,
    /// Whether to auto-approve tool calls by default.
    pub auto_mode: Option<bool>,
}

/// Configuration section for CodeQL static analysis.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CodeqlConfigSection {
    pub enabled: Option<bool>,
    pub query_packs: Option<Vec<String>>,
    pub max_results: Option<usize>,
    pub timeout_secs: Option<u64>,
    pub language: Option<String>,
    pub database_path: Option<String>,
}

/// Model routing configuration from [agent.models] TOML section.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelsSection {
    pub thinking: Option<String>,
    pub fast: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentSection {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub max_iterations: Option<usize>,
    pub models: Option<ModelsSection>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    let approval = merge_approval(
        user.as_ref().and_then(|c| c.approval.as_ref()),
        project.as_ref().and_then(|c| c.approval.as_ref()),
    );
    let auto_mode = project
        .as_ref()
        .and_then(|c| c.auto_mode)
        .or_else(|| user.as_ref().and_then(|c| c.auto_mode));
    let codeql = merge_codeql(
        user.as_ref().and_then(|c| c.codeql.as_ref()),
        project.as_ref().and_then(|c| c.codeql.as_ref()),
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

    FileConfig { agent, approval, codeql, env, auto_mode }
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
            models: u.models.clone(),
        }),
        (None, Some(p)) => Some(AgentSection {
            model: p.model.clone(),
            provider: p.provider.clone(),
            max_iterations: p.max_iterations,
            models: p.models.clone(),
        }),
        (Some(u), Some(p)) => Some(AgentSection {
            model: p.model.clone().or_else(|| u.model.clone()),
            provider: p.provider.clone().or_else(|| u.provider.clone()),
            max_iterations: p.max_iterations.or(u.max_iterations),
            models: p.models.clone().or_else(|| u.models.clone()),
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

fn merge_codeql(
    user: Option<&CodeqlConfigSection>,
    project: Option<&CodeqlConfigSection>,
) -> Option<CodeqlConfigSection> {
    match (user, project) {
        (None, None) => None,
        (Some(u), None) => Some(u.clone()),
        (None, Some(p)) => Some(p.clone()),
        (Some(u), Some(p)) => Some(CodeqlConfigSection {
            enabled: p.enabled.or(u.enabled),
            query_packs: p.query_packs.clone().or_else(|| u.query_packs.clone()),
            max_results: p.max_results.or(u.max_results),
            timeout_secs: p.timeout_secs.or(u.timeout_secs),
            language: p.language.clone().or_else(|| u.language.clone()),
            database_path: p.database_path.clone().or_else(|| u.database_path.clone()),
        }),
    }
}

pub enum ResolvedProvider {
    DeepSeek(DeepSeekProvider),
    Routed(RoutedProvider),
    Mock(MockProvider),
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

    // Priority: CLI --max-iterations > config file > default 8
    agent_config.max_iterations =
        options.max_iterations.or_else(|| config.agent.as_ref()?.max_iterations).unwrap_or(30);

    agent_config.auto_validate_schema = !options.no_validate_schema;
    agent_config.approval_handler = approval_handler;

    // Inherit a safe subset of the process environment (PATH, HOME),
    // then merge env vars from FileConfig (may include tool configs, API keys).
    let mut env = HashMap::new();
    for key in ["PATH", "HOME"] {
        if let Ok(value) = std::env::var(key) {
            env.insert(key.to_string(), value);
        }
    }
    if let Some(config_env) = &config.env {
        for (k, v) in config_env {
            env.insert(k.clone(), v.clone());
        }
    }
    agent_config.env = env;

    Ok(agent_config)
}

pub fn build_provider(options: &SharedOptions, config: &FileConfig) -> Result<ResolvedProvider> {
    // Priority: CLI --provider > TELOS_PROVIDER env (already in options) > config file > Mock
    let provider =
        options.provider.or_else(|| provider_from_config(config)).unwrap_or(ProviderArg::Mock);

    let config_env = config.env.as_ref();

    match provider {
        ProviderArg::Deepseek => {
            // --model overrides both thinking and fast (single-model mode)
            let explicit_model =
                options.model.clone().or_else(|| config.agent.as_ref()?.model.clone());

            // Default: thinking = deepseek-v4-pro, fast = deepseek-v4-flash
            // Individual overrides via --thinking-model / --fast-model or config
            let thinking_model = options
                .thinking_model
                .clone()
                .or_else(|| config.agent.as_ref()?.models.as_ref()?.thinking.clone())
                .or_else(|| explicit_model.clone())
                .unwrap_or_else(|| "deepseek-v4-pro".into());

            let fast_model = options
                .fast_model
                .clone()
                .or_else(|| config.agent.as_ref()?.models.as_ref()?.fast.clone())
                .or(explicit_model)
                .unwrap_or_else(|| "deepseek-v4-flash".into());

            let api_key =
                resolve_api_key(provider, options.api_key.clone(), config_env, "DEEPSEEK_API_KEY")?;

            if thinking_model != fast_model {
                let routed_config = RoutedModelConfig::dual(api_key, thinking_model, fast_model);
                Ok(ResolvedProvider::Routed(RoutedProvider::new(routed_config)))
            } else {
                let cfg = DeepSeekConfig::new(api_key, thinking_model);
                Ok(ResolvedProvider::DeepSeek(DeepSeekProvider::new(cfg)))
            }
        }
        ProviderArg::Mock => Ok(ResolvedProvider::Mock(MockProvider::new(vec![]))),
    }
}

/// Parse a provider string from FileConfig into a ProviderArg.
fn provider_from_config(config: &FileConfig) -> Option<ProviderArg> {
    let s = config.agent.as_ref()?.provider.as_deref()?;
    match s.to_lowercase().as_str() {
        "deepseek" | "deep" => Some(ProviderArg::Deepseek),
        "mock" => Some(ProviderArg::Mock),
        _ => None,
    }
}

/// Build a provider directly from onboarding results, bypassing all
/// CLI/env/config resolution. Used when the user just completed setup.
pub fn build_provider_from_onboarding(result: &OnboardingResult) -> Result<ResolvedProvider> {
    match result.provider {
        ProviderArg::Deepseek => {
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
        ProviderArg::Mock => Ok(ResolvedProvider::Mock(MockProvider::new(vec![]))),
    }
}

/// Apply env vars from FileConfig to the process environment.
/// Does NOT override already-set vars — CLI/env vars from outside the config
/// take priority.
///
/// Apply env vars from FileConfig to the process environment.
///
/// # Deprecation
/// This function is a no-op and will be removed in a future version.
/// Config env vars are now read directly from `FileConfig::env`
/// instead of being mirrored into the process environment.
/// Use [`resolve_api_key`] or read `FileConfig::env` directly instead.
#[deprecated(
    since = "0.2.0",
    note = "config env vars are now read directly from FileConfig::env; apply_config_env is a no-op and will be removed"
)]
pub fn apply_config_env(_config: &FileConfig) {
    // No-op: avoid unsafe `std::env::set_var` in multi-threaded async contexts.
}

fn resolve_api_key(
    provider: ProviderArg,
    cli_key: Option<String>,
    config_env: Option<&HashMap<String, String>>,
    env_var: &str,
) -> Result<String> {
    if let Some(key) = cli_key {
        return Ok(key);
    }

    // Process environment variable takes priority over config file env.
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

fn provider_name(provider: ProviderArg) -> &'static str {
    match provider {
        ProviderArg::Deepseek => "DeepSeek",
        ProviderArg::Mock => "Mock",
    }
}

pub fn default_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_provider_defaults_to_mock_when_no_config() {
        let options = SharedOptions::default();
        let config = FileConfig::default();
        let result = build_provider(&options, &config).unwrap();
        assert!(matches!(result, ResolvedProvider::Mock(_)));
    }

    #[test]
    fn build_provider_reads_provider_from_file_config() {
        let options = SharedOptions { api_key: Some("sk-test".into()), ..Default::default() };
        let config = FileConfig {
            agent: Some(AgentSection {
                provider: Some("deepseek".into()),
                model: Some("deepseek-chat".into()),
                max_iterations: None,
                models: None,
            }),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        assert!(matches!(result, ResolvedProvider::DeepSeek(_)));
    }

    #[test]
    fn build_provider_cli_flag_overrides_file_config() {
        let options = SharedOptions { provider: Some(ProviderArg::Mock), ..Default::default() };
        let config = FileConfig {
            agent: Some(AgentSection { provider: Some("deepseek".into()), ..Default::default() }),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        assert!(matches!(result, ResolvedProvider::Mock(_)));
    }

    #[test]
    fn build_agent_config_merges_env_from_file_config() {
        let options = SharedOptions::default();
        let config = FileConfig {
            env: Some(HashMap::from([("CUSTOM_VAR".into(), "value".into())])),
            ..FileConfig::default()
        };
        let agent = build_agent_config(&options, &config, None).unwrap();
        assert_eq!(agent.env.get("CUSTOM_VAR").map(|s| s.as_str()), Some("value"));
        assert!(agent.env.contains_key("PATH")); // base env preserved
    }

    #[test]
    fn build_agent_config_uses_config_max_iterations() {
        let options = SharedOptions::default(); // max_iterations is None (Option)
        let config = FileConfig {
            agent: Some(AgentSection { max_iterations: Some(5), ..Default::default() }),
            ..FileConfig::default()
        };
        let agent = build_agent_config(&options, &config, None).unwrap();
        assert_eq!(agent.max_iterations, 5);
    }

    #[test]
    fn build_agent_config_cli_max_iterations_overrides_config() {
        let options = SharedOptions { max_iterations: Some(12), ..Default::default() };
        let config = FileConfig {
            agent: Some(AgentSection { max_iterations: Some(5), ..Default::default() }),
            ..FileConfig::default()
        };
        let agent = build_agent_config(&options, &config, None).unwrap();
        assert_eq!(agent.max_iterations, 12);
    }

    #[test]
    fn build_agent_config_defaults_max_iterations_to_30() {
        let options = SharedOptions::default();
        let config = FileConfig::default();
        let agent = build_agent_config(&options, &config, None).unwrap();
        assert_eq!(agent.max_iterations, 30);
    }

    #[test]
    fn provider_from_config_parses_variants() {
        fn p(s: &str) -> Option<ProviderArg> {
            let config = FileConfig {
                agent: Some(AgentSection { provider: Some(s.into()), ..Default::default() }),
                ..FileConfig::default()
            };
            provider_from_config(&config)
        }
        assert!(matches!(p("deepseek"), Some(ProviderArg::Deepseek)));
        assert!(matches!(p("deep"), Some(ProviderArg::Deepseek)));
        assert!(matches!(p("mock"), Some(ProviderArg::Mock)));
        assert!(p("unknown").is_none());
        assert!(p("").is_none());
    }

    #[test]
    fn build_provider_reads_api_key_from_config_env() {
        let options = SharedOptions::default();
        let config = FileConfig {
            agent: Some(AgentSection { provider: Some("deepseek".into()), ..Default::default() }),
            env: Some(HashMap::from([("DEEPSEEK_API_KEY".into(), "sk-from-config".into())])),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        assert!(!matches!(result, ResolvedProvider::Mock(_)), "should resolve to a real provider");
    }

    #[test]
    fn cli_api_key_overrides_config_env() {
        let options = SharedOptions {
            provider: Some(ProviderArg::Deepseek),
            api_key: Some("sk-from-cli".into()),
            ..Default::default()
        };
        let config = FileConfig {
            env: Some(HashMap::from([("DEEPSEEK_API_KEY".into(), "sk-from-config".into())])),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        assert!(!matches!(result, ResolvedProvider::Mock(_)), "should resolve to a real provider");
    }

    #[test]
    fn build_provider_with_dual_models_creates_routed() {
        let options = SharedOptions {
            api_key: Some("sk-test".into()),
            thinking_model: Some("deepseek-v4-pro".into()),
            fast_model: Some("deepseek-v4-flash".into()),
            ..Default::default()
        };
        let config = FileConfig {
            agent: Some(AgentSection { provider: Some("deepseek".into()), ..Default::default() }),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        assert!(matches!(result, ResolvedProvider::Routed(_)));
    }

    #[test]
    fn build_provider_with_same_models_creates_plain_deepseek() {
        let options = SharedOptions {
            api_key: Some("sk-test".into()),
            thinking_model: Some("deepseek-v4-pro".into()),
            fast_model: Some("deepseek-v4-pro".into()),
            ..Default::default()
        };
        let config = FileConfig {
            agent: Some(AgentSection { provider: Some("deepseek".into()), ..Default::default() }),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        assert!(matches!(result, ResolvedProvider::DeepSeek(_)));
    }

    #[test]
    fn build_provider_without_model_flags_creates_routed_by_default() {
        let options = SharedOptions { api_key: Some("sk-test".into()), ..Default::default() };
        let config = FileConfig {
            agent: Some(AgentSection { provider: Some("deepseek".into()), ..Default::default() }),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        // Default: thinking=pro, fast=flash → Routed
        assert!(matches!(result, ResolvedProvider::Routed(_)));
    }

    #[test]
    fn build_provider_with_explicit_model_creates_plain_deepseek() {
        let options = SharedOptions {
            api_key: Some("sk-test".into()),
            model: Some("deepseek-v4-flash".into()),
            ..Default::default()
        };
        let config = FileConfig {
            agent: Some(AgentSection { provider: Some("deepseek".into()), ..Default::default() }),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        // --model overrides both → same model → plain DeepSeek
        assert!(matches!(result, ResolvedProvider::DeepSeek(_)));
    }
}
