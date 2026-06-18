pub mod cli;
pub mod config;
pub mod project;
pub mod runner;
pub mod terminal;

pub mod approval;
pub mod display;
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

    let approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>> =
        if std::io::stdin().is_terminal() {
            Some(Arc::new(approval::TerminalApprovalHandler::default()))
        } else {
            None
        };

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
