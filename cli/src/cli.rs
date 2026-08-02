use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProviderArg {
    Deepseek,
    Mock,
}

#[derive(Debug, Parser)]
#[clap(
    name = "telos",
    about = "Terminal interface for telos-agent",
    version,
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
    /// JSON-line daemon mode: read commands from stdin, emit events on stdout.
    Serve,
    /// Manage plugins and marketplaces without starting a model provider.
    Plugin {
        #[clap(subcommand)]
        command: PluginCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    List,
    Inspect {
        id: String,
    },
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    Install {
        id: String,
    },
    InstallLocal {
        path: PathBuf,
        #[clap(long, default_value = "local")]
        marketplace: String,
    },
    Upgrade {
        id: String,
    },
    Uninstall {
        id: String,
    },
    Config {
        id: String,
        #[clap(long)]
        json: String,
    },
    ClearConfig {
        id: String,
    },
    MarketplaceAddLocal {
        path: PathBuf,
        #[clap(long)]
        name: Option<String>,
    },
    MarketplaceAddGithub {
        repo: String,
        #[clap(long = "ref")]
        ref_: Option<String>,
        #[clap(long)]
        path: Option<String>,
        #[clap(long)]
        name: Option<String>,
    },
    MarketplaceRefresh {
        name: String,
    },
    MarketplaceRemove {
        name: String,
    },
    MarketplaceSearch {
        query: String,
    },
    MarketplaceListPlugins {
        #[clap(long)]
        name: Option<String>,
    },
    MarketplaceList,
}

#[derive(Debug, Parser, Clone, Default)]
pub struct SharedOptions {
    /// Model provider to use.
    #[clap(long, value_enum, env = "TELOS_PROVIDER")]
    pub provider: Option<ProviderArg>,

    /// Model name.
    #[clap(long, env = "TELOS_MODEL")]
    pub model: Option<String>,

    /// Model name for the thinking/reasoning model (planning, complex decisions).
    #[clap(long, env = "TELOS_THINKING_MODEL")]
    pub thinking_model: Option<String>,

    /// Model name for the fast/execution model (tool calls, file ops, simple tasks).
    #[clap(long, env = "TELOS_FAST_MODEL")]
    pub fast_model: Option<String>,

    /// API key for the selected provider.
    #[clap(long, env = "TELOS_API_KEY")]
    pub api_key: Option<String>,

    /// Working directory for filesystem and shell tools.
    #[clap(long, env = "TELOS_CWD")]
    pub cwd: Option<PathBuf>,

    /// Optional maximum number of model-tool iterations per turn.
    #[clap(long)]
    pub max_iterations: Option<usize>,

    /// Disable automatic JSON schema validation of tool arguments.
    #[clap(long)]
    pub no_validate_schema: bool,

    /// Reduce output.
    #[clap(short, long)]
    pub quiet: bool,

    /// Increase output verbosity.
    #[clap(short, long)]
    pub verbose: bool,

    /// Path to a config file to load.
    #[clap(long, env = "TELOS_CONFIG", global = true)]
    pub config: Option<PathBuf>,
}

impl SharedOptions {
    pub fn to_runtime(&self) -> telos_agent_host::SharedOptions {
        telos_agent_host::SharedOptions {
            provider: self.provider.map(|provider| match provider {
                ProviderArg::Deepseek => telos_agent_host::ProviderKind::Deepseek,
                ProviderArg::Mock => telos_agent_host::ProviderKind::Mock,
            }),
            model: self.model.clone(),
            thinking_model: self.thinking_model.clone(),
            fast_model: self.fast_model.clone(),
            api_key: self.api_key.clone(),
            cwd: self.cwd.clone(),
            max_iterations: self.max_iterations,
            no_validate_schema: self.no_validate_schema,
        }
    }
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

    #[test]
    fn parse_plugin_management_commands() {
        let cli = Cli::parse_from(["telos", "plugin", "install", "formatter@community"]);
        assert!(matches!(
            cli.command,
            Some(Command::Plugin {
                command: PluginCommand::Install { id }
            }) if id == "formatter@community"
        ));

        let cli = Cli::parse_from([
            "telos",
            "plugin",
            "marketplace-add-github",
            "org/catalog",
            "--ref",
            "stable",
            "--path",
            "registry",
        ]);
        assert!(matches!(
            cli.command,
            Some(Command::Plugin {
                command: PluginCommand::MarketplaceAddGithub { repo, ref_, path, name: _ }
            }) if repo == "org/catalog" && ref_.as_deref() == Some("stable") && path.as_deref() == Some("registry")
        ));

        let cli = Cli::parse_from(["telos", "plugin", "marketplace-search", "format"]);
        assert!(matches!(
            cli.command,
            Some(Command::Plugin {
                command: PluginCommand::MarketplaceSearch { query }
            }) if query == "format"
        ));

        assert!(
            Cli::try_parse_from([
                "telos",
                "plugin",
                "marketplace-add-url",
                "https://example.com/marketplace.json",
            ])
            .is_err()
        );
    }
}
