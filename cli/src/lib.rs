pub mod cli;
pub mod config;
pub mod project;
pub mod runner;
pub mod terminal;
pub mod tui;

pub mod approval;
pub mod repl;
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
            generate_completion(shell);
            Ok(())
        }
        Some(Command::Chat) => runner::run_chat(&cli.shared, approval_handler).await,
        None => {
            let prompt = cli.prompt.join(" ");
            if prompt.trim().is_empty() {
                anyhow::bail!("Usage: telos [OPTIONS] <PROMPT>");
            }
            runner::run_single(&cli.shared, prompt, approval_handler).await
        }
    }
}

fn generate_completion(shell: clap_complete::Shell) {
    let mut cmd = <Cli as clap::CommandFactory>::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
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
