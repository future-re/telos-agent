#[allow(deprecated)]
pub use telos_runtime::config::{
    AgentSection, ApprovalSection, BillingModelPricing, BillingSection, DefaultShell,
    DiagnosticsGithubSection, DiagnosticsSection, FileConfig, ModelsSection, ResolvedProvider,
    TuiDensity, TuiSection, apply_config_env, default_cwd, load_config_file, load_project_config,
    load_user_config, merge_configs,
};

use std::sync::Arc;

use anyhow::Result;
use telos_runtime::{ProviderKind, ProviderSetup};

pub fn build_agent_config(
    options: &crate::cli::SharedOptions,
    config: &FileConfig,
    approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>>,
) -> Result<telos_agent::AgentConfig> {
    telos_runtime::config::build_agent_config(&options.to_runtime(), config, approval_handler)
}

pub fn build_provider(
    options: &crate::cli::SharedOptions,
    config: &FileConfig,
) -> Result<ResolvedProvider> {
    telos_runtime::config::build_provider(&options.to_runtime(), config)
}

pub fn build_provider_from_onboarding(
    result: &crate::onboarding::OnboardingResult,
) -> Result<ResolvedProvider> {
    let setup = ProviderSetup {
        provider: match result.provider {
            crate::cli::ProviderArg::Deepseek => ProviderKind::Deepseek,
            crate::cli::ProviderArg::Mock => ProviderKind::Mock,
        },
        api_key: result.api_key.clone(),
        thinking_model: result.thinking_model.clone(),
        fast_model: result.fast_model.clone(),
    };
    telos_runtime::config::build_provider_from_setup(&setup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{ProviderArg, SharedOptions};
    use std::collections::HashMap;

    #[test]
    fn build_provider_defaults_to_mock_when_no_config() {
        let options = SharedOptions::default();
        let config = FileConfig::default();
        let result = build_provider(&options, &config).unwrap();
        assert!(matches!(result, ResolvedProvider::Mock(_)));
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
    }
}
