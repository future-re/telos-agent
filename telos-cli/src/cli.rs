use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProviderArg {
    Kimi,
    Deepseek,
    Mock,
}

#[derive(Debug, Parser)]
#[clap(
    name = "telos",
    about = "Terminal interface for telos-agent",
    version,
    subcommand_negates_reqs = true,
    override_usage = "telos [OPTIONS] [PROMPT]\n       telos [OPTIONS] <COMMAND>"
)]
pub struct Cli {
    #[clap(flatten)]
    pub shared: SharedOptions,

    /// Prompt to send to the agent. All positional arguments are joined with spaces.
    #[clap(value_name = "PROMPT")]
    pub prompt: Vec<String>,

    #[clap(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start an interactive REPL session.
    Chat,
    /// Generate shell completion scripts.
    Completion {
        #[clap(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Debug, Parser, Clone)]
pub struct SharedOptions {
    /// Model provider to use.
    #[clap(long, value_enum, env = "TELOS_PROVIDER", global = true)]
    pub provider: Option<ProviderArg>,

    /// Model name.
    #[clap(long, env = "TELOS_MODEL", global = true)]
    pub model: Option<String>,

    /// API key for the selected provider.
    #[clap(long, env = "TELOS_API_KEY", global = true)]
    pub api_key: Option<String>,

    /// Working directory for filesystem and shell tools.
    #[clap(long, env = "TELOS_CWD", global = true)]
    pub cwd: Option<PathBuf>,

    /// Maximum number of model-tool iterations per turn.
    #[clap(long, default_value = "8", global = true)]
    pub max_iterations: usize,

    /// Disable automatic JSON schema validation of tool arguments.
    #[clap(long, global = true)]
    pub no_validate_schema: bool,

    /// Reduce output.
    #[clap(short, long, global = true)]
    pub quiet: bool,

    /// Increase output verbosity.
    #[clap(short, long, global = true)]
    pub verbose: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_prompt() {
        let cli = Cli::parse_from(["telos", "hello"]);
        assert!(cli.command.is_none());
        assert_eq!(cli.shared.provider, None);
        assert_eq!(cli.prompt, vec!["hello"]);
    }

    #[test]
    fn parse_provider_flag() {
        let cli = Cli::parse_from(["telos", "--provider", "mock", "do it"]);
        assert!(matches!(cli.shared.provider, Some(ProviderArg::Mock)));
    }

    #[test]
    fn parse_chat_command() {
        let cli = Cli::parse_from(["telos", "chat"]);
        assert!(matches!(cli.command, Some(Command::Chat)));
    }
}
