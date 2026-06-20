mod load;
mod merge;
mod provider;
mod types;

pub use load::{default_cwd, load_config_file, load_project_config, load_user_config};
pub use merge::merge_configs;
#[allow(deprecated)]
pub use provider::apply_config_env;
pub use provider::{
    ResolvedProvider, build_agent_config, build_provider, build_provider_from_onboarding,
};
pub use types::{
    AgentSection, ApprovalSection, DefaultShell, DiagnosticsGithubSection, DiagnosticsSection,
    FileConfig, ModelsSection, TuiDensity, TuiSection,
};

#[cfg(test)]
use provider::{DeepSeekModelSelection, provider_from_config, resolve_deepseek_model_selection};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::cli::{ProviderArg, SharedOptions};

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
                default_shell: None,
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
    fn parses_diagnostics_config() {
        let cfg: FileConfig = toml::from_str(
            r#"
[diagnostics]
enabled = true
retention_days = 7

[diagnostics.github]
enabled = true
repository = "future-re/telos-agent"
interval_hours = 12
min_occurrences = 2
"#,
        )
        .unwrap();
        let diagnostics = cfg.diagnostics.unwrap();
        assert_eq!(diagnostics.enabled, Some(true));
        assert_eq!(diagnostics.retention_days, Some(7));
        let github = diagnostics.github.unwrap();
        assert_eq!(github.enabled, Some(true));
        assert_eq!(github.repository.as_deref(), Some("future-re/telos-agent"));
        assert_eq!(github.interval_hours, Some(12));
        assert_eq!(github.min_occurrences, Some(2));
    }

    #[test]
    fn parses_tui_density_config() {
        let cfg: FileConfig = toml::from_str(
            r#"
[tui]
density = "compact"
"#,
        )
        .unwrap();

        assert_eq!(cfg.tui.unwrap().density, Some(TuiDensity::Compact));
    }

    #[test]
    fn parses_agent_default_shell() {
        let cfg: FileConfig = toml::from_str(
            r#"
[agent]
default_shell = "powershell"
"#,
        )
        .unwrap();

        assert_eq!(cfg.agent.unwrap().default_shell, Some(DefaultShell::PowerShell));
    }

    #[test]
    fn rejects_invalid_tui_density_config() {
        let err = toml::from_str::<FileConfig>(
            r#"
[tui]
density = "cozy"
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("unknown variant"));
    }

    #[test]
    fn merge_configs_project_tui_density_overrides_user() {
        let user = FileConfig {
            tui: Some(TuiSection { density: Some(TuiDensity::Compact) }),
            ..FileConfig::default()
        };
        let project = FileConfig {
            tui: Some(TuiSection { density: Some(TuiDensity::Spacious) }),
            ..FileConfig::default()
        };

        let merged = merge_configs(Some(user), Some(project));

        assert_eq!(merged.tui.unwrap().density, Some(TuiDensity::Spacious));
    }

    #[test]
    fn merge_configs_project_missing_tui_density_falls_back_to_user() {
        let user = FileConfig {
            tui: Some(TuiSection { density: Some(TuiDensity::Compact) }),
            ..FileConfig::default()
        };
        let project =
            FileConfig { tui: Some(TuiSection { density: None }), ..FileConfig::default() };

        let merged = merge_configs(Some(user), Some(project));

        assert_eq!(merged.tui.unwrap().density, Some(TuiDensity::Compact));
    }

    #[test]
    fn merge_configs_project_default_shell_overrides_user() {
        let user = FileConfig {
            agent: Some(AgentSection {
                default_shell: Some(DefaultShell::Bash),
                ..Default::default()
            }),
            ..FileConfig::default()
        };
        let project = FileConfig {
            agent: Some(AgentSection {
                default_shell: Some(DefaultShell::PowerShell),
                ..Default::default()
            }),
            ..FileConfig::default()
        };

        let merged = merge_configs(Some(user), Some(project));

        assert_eq!(merged.agent.unwrap().default_shell, Some(DefaultShell::PowerShell));
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

    #[test]
    fn model_auto_selects_routed_deepseek_models() {
        let options = SharedOptions { model: Some("auto".into()), ..Default::default() };
        let config = FileConfig {
            agent: Some(AgentSection { provider: Some("deepseek".into()), ..Default::default() }),
            ..FileConfig::default()
        };

        let selection = resolve_deepseek_model_selection(&options, &config);

        assert_eq!(
            selection,
            DeepSeekModelSelection::Routed {
                thinking: "deepseek-v4-pro".into(),
                fast: "deepseek-v4-flash".into(),
            }
        );
    }

    #[test]
    fn model_pro_selects_single_pro_model() {
        let options = SharedOptions {
            model: Some("pro".into()),
            thinking_model: Some("deepseek-v4-flash".into()),
            fast_model: Some("deepseek-v4-flash".into()),
            ..Default::default()
        };
        let config = FileConfig {
            agent: Some(AgentSection { provider: Some("deepseek".into()), ..Default::default() }),
            ..FileConfig::default()
        };

        let selection = resolve_deepseek_model_selection(&options, &config);

        assert_eq!(selection, DeepSeekModelSelection::Single("deepseek-v4-pro".into()));
    }
}
