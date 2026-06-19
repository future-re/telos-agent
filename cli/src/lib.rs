pub mod cli;
pub mod codeql_runtime;
pub mod config;
pub mod context;
pub mod diagnostics;
pub mod memory_runtime;
pub mod project;
pub mod runner;
pub mod terminal;
pub mod tui;

pub mod approval;
pub mod onboarding;
pub mod session;

pub use project::find_project_root;

use std::io::IsTerminal;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

/// Entry point shared between the binary and integration tests.
pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    // ── Load and merge config layers ──────────────────────────────────────
    let user_config = config::load_user_config(cli.shared.config.as_deref())?;
    let project_root =
        project::find_project_root(cli.shared.cwd.as_deref().unwrap_or(&std::env::current_dir()?))
            .ok();
    let project_config = match &project_root {
        Some(root) => config::load_project_config(root)?,
        None => None,
    };
    let merged = config::merge_configs(user_config, project_config);

    // Print project root on startup (informational).
    if let Some(ref root) = project_root {
        eprintln!("Project root: {}", root.display());
    }

    // ── Build approval handler with policy from config ─────────────────────
    let approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>> =
        if std::io::stdin().is_terminal() {
            let policy = merged.approval.as_ref().map(|a| {
                let default: approval::ApprovalPolicy =
                    a.default_policy.as_deref().and_then(parse_policy_str).unwrap_or_default();
                let mut policies = std::collections::HashMap::new();
                if let Some(ref map) = a.policies {
                    for (tool, pol) in map {
                        if let Some(p) = parse_policy_str(pol) {
                            policies.insert(tool.clone(), p);
                        }
                    }
                }
                approval::PolicyConfig { default, policies }
            });

            Some(Arc::new(approval::TerminalApprovalHandler::new(policy)))
        } else {
            None
        };

    // ── Dispatch ───────────────────────────────────────────────────────────
    match cli.command {
        Some(Command::Completion { shell }) => {
            // Completion subcommand doesn't need a provider — skip onboarding.
            generate_completion(shell);
            Ok(())
        }
        Some(Command::Chat) => {
            let onboarding = match check_onboarding(&cli.shared, &merged) {
                Ok(o) => o,
                Err(e) => {
                    if e.to_string().contains("Setup cancelled") {
                        return Ok(());
                    }
                    return Err(e);
                }
            };
            runner::run_chat(&cli.shared, &merged, onboarding, approval_handler).await
        }
        None => {
            let prompt = cli.prompt.join(" ");
            if prompt.trim().is_empty() {
                let onboarding = match check_onboarding(&cli.shared, &merged) {
                    Ok(o) => o,
                    Err(e) => {
                        if e.to_string().contains("Setup cancelled") {
                            return Ok(());
                        }
                        return Err(e);
                    }
                };
                let config =
                    config::build_agent_config(&cli.shared, &merged, approval_handler.clone())?;
                let provider = if let Some(ref onb) = onboarding {
                    build_erased_from_onboarding(onb)?
                } else {
                    build_erased_provider(&cli.shared, &merged)?
                };
                let mut tools = telos_agent::ToolRegistry::new();
                telos_agent::register_core_tools(&mut tools);

                let current_dir = std::env::current_dir()?;
                let cwd = cli.shared.cwd.as_deref().unwrap_or(&current_dir);
                let project_root = project::find_project_root(cwd).ok();
                runner::register_cli_task_tools(&mut tools, project_root.as_deref().unwrap_or(cwd));
                let ctx = match &project_root {
                    Some(root) => crate::context::load_project_context(root),
                    None => crate::context::ProjectContext::empty(),
                };

                // Inject the loaded context into the agent's prompt assembly.
                let memory_store =
                    crate::memory_runtime::open_memory_store(project_root.as_deref())?;
                let mut assembly = crate::context::build_prompt_assembly(&ctx);
                crate::memory_runtime::register_memory_runtime(
                    &mut tools,
                    &mut assembly,
                    memory_store.clone(),
                );
                // CodeQL startup analysis (background).
                let codeql_cfg = crate::codeql_runtime::codeql_config_from_file(&merged);
                let codeql_runtime = crate::codeql_runtime::register_codeql(
                    &mut tools,
                    &mut assembly,
                    memory_store.clone(),
                    codeql_cfg,
                    project_root
                        .clone()
                        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
                );
                if let Some(runtime) = codeql_runtime {
                    tokio::spawn(async move {
                        let report = runtime.run_startup_analysis().await;
                        tracing::info!(?report, "CodeQL startup analysis complete");
                    });
                }
                let mut tui_config = config;
                let _diagnostics_runtime = crate::diagnostics::configure_tool_diagnostics(
                    &mut tui_config,
                    &merged,
                    project_root.as_deref(),
                )?;
                tui_config.prompt_assembly = Some(Arc::new(assembly));

                let model_display =
                    cli.shared.model.as_deref().or_else(|| merged.agent.as_ref()?.model.as_deref());
                let status =
                    crate::context::build_status_text(model_display, project_root.as_deref(), &ctx);

                let result = tui::run(
                    tui_config,
                    provider,
                    tools,
                    status,
                    project_root.as_deref(),
                    project_root.as_deref().unwrap_or(cwd),
                    merged.auto_mode.unwrap_or(false),
                    memory_store,
                    tui::app::ModelSwitchConfig {
                        deepseek_api_key: deepseek_api_key_for_switch(
                            &cli.shared,
                            &merged,
                            onboarding.as_ref(),
                        ),
                    },
                    tui::app::TuiLayoutSettings::from_density(
                        merged.tui.as_ref().and_then(|tui| tui.density).unwrap_or_default(),
                    ),
                )
                .await;
                if let Some(runtime) = &_diagnostics_runtime
                    && let Err(err) =
                        crate::diagnostics::process_diagnostics(runtime, &merged).await
                {
                    tracing::warn!("failed to process diagnostics: {err}");
                }
                return result;
            }
            let onboarding = match check_onboarding(&cli.shared, &merged) {
                Ok(o) => o,
                Err(e) => {
                    if e.to_string().contains("Setup cancelled") {
                        return Ok(());
                    }
                    return Err(e);
                }
            };
            runner::run_single(&cli.shared, &merged, onboarding, prompt, approval_handler).await
        }
    }
}

/// Check whether a provider is configured. If not, and stdin is a terminal,
/// launch the interactive onboarding wizard. Returns `None` if the provider was
/// already configured (no onboarding needed), or `Some(result)` if the user
/// completed the setup wizard.
fn check_onboarding(
    options: &cli::SharedOptions,
    merged: &config::FileConfig,
) -> Result<Option<onboarding::OnboardingResult>> {
    let has_provider = options.provider.is_some()
        || std::env::var("TELOS_PROVIDER").is_ok()
        || merged.agent.as_ref().and_then(|a| a.provider.as_ref()).is_some();

    if has_provider {
        return Ok(None);
    }

    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "No provider configured.\n\
             Set TELOS_PROVIDER and TELOS_API_KEY environment variables,\n\
             or create ~/.config/telos/config.toml with:\n\
             \n  [agent]\n  provider = \"deepseek\"\n\
             \n  Or run `telos` interactively to use the setup wizard."
        );
    }

    match onboarding::run() {
        Ok(Some(result)) => Ok(Some(result)),
        Ok(None) => {
            eprintln!("\nSetup cancelled. Exiting.");
            // Return Ok(()) would exit run(), but we're in a helper. The caller
            // checks for this and should propagate. We use a custom error that the caller catches.
            anyhow::bail!("Setup cancelled");
        }
        Err(e) => Err(e),
    }
}

fn generate_completion(shell: clap_complete::Shell) {
    let mut cmd = <Cli as clap::CommandFactory>::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
}

pub(crate) fn deepseek_api_key_for_switch(
    options: &cli::SharedOptions,
    config: &config::FileConfig,
    onboarding: Option<&onboarding::OnboardingResult>,
) -> Option<String> {
    options
        .api_key
        .clone()
        .or_else(|| onboarding.map(|result| result.api_key.clone()))
        .or_else(|| std::env::var("DEEPSEEK_API_KEY").ok())
        .or_else(|| config.env.as_ref()?.get("DEEPSEEK_API_KEY").cloned())
        .filter(|key| !key.trim().is_empty())
}

pub(crate) fn build_erased_provider(
    options: &cli::SharedOptions,
    config: &config::FileConfig,
) -> Result<Arc<dyn telos_agent::ModelProvider>> {
    match config::build_provider(options, config)? {
        config::ResolvedProvider::DeepSeek(p) => Ok(Arc::new(p)),
        config::ResolvedProvider::Routed(p) => Ok(Arc::new(p)),
        config::ResolvedProvider::Mock(p) => Ok(Arc::new(p)),
    }
}

pub(crate) fn build_erased_from_onboarding(
    onb: &onboarding::OnboardingResult,
) -> Result<Arc<dyn telos_agent::ModelProvider>> {
    match config::build_provider_from_onboarding(onb)? {
        config::ResolvedProvider::DeepSeek(p) => Ok(Arc::new(p)),
        config::ResolvedProvider::Routed(p) => Ok(Arc::new(p)),
        config::ResolvedProvider::Mock(p) => Ok(Arc::new(p)),
    }
}

/// Parse a policy string from config ("allow", "ask", "deny") into
/// [`approval::ApprovalPolicy`]. Returns `None` for unrecognized values.
fn parse_policy_str(s: &str) -> Option<approval::ApprovalPolicy> {
    match s.to_lowercase().as_str() {
        "allow" | "always-allow" | "always_allow" => Some(approval::ApprovalPolicy::AlwaysAllow),
        "ask" | "always-ask" | "always_ask" => Some(approval::ApprovalPolicy::AlwaysAsk),
        "deny" | "always-deny" | "always_deny" => Some(approval::ApprovalPolicy::AlwaysDeny),
        _ => None,
    }
}
