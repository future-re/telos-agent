pub mod cli;
pub mod config;
pub mod runner;
pub mod terminal;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

/// Entry point shared between the binary and integration tests.
pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Completion { shell }) => {
            generate_completion(shell);
            Ok(())
        }
        Some(Command::Chat) => {
            eprintln!("Interactive chat is not yet implemented; use `telos <PROMPT>`.");
            Ok(())
        }
        None => {
            let prompt = cli.prompt.join(" ");
            if prompt.trim().is_empty() {
                anyhow::bail!("Usage: telos [OPTIONS] <PROMPT>");
            }
            runner::run_single(&cli.shared, prompt).await
        }
    }
}

fn generate_completion(shell: clap_complete::Shell) {
    let mut cmd = <Cli as clap::CommandFactory>::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
}
