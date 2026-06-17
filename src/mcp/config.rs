use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration for one MCP server.
///
/// Controls how the server process is spawned and how the client interacts
/// with it.
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// The command to spawn (e.g. "npx", "uvx", "node").
    pub command: String,
    /// Arguments passed to the command.
    pub args: Vec<String>,
    /// Additional environment variables passed to the child process.
    pub env: HashMap<String, String>,
    /// Working directory for the child process.
    pub cwd: Option<PathBuf>,
    /// Auto-connect on session start.
    pub auto_connect: bool,
    /// Request timeout in milliseconds.
    pub timeout_ms: u64,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            auto_connect: true,
            timeout_ms: 60_000,
        }
    }
}

impl McpServerConfig {
    /// Create a new config with the given command and arguments.
    ///
    /// All other fields use their defaults.
    pub fn new(command: &str, args: Vec<String>) -> Self {
        Self { command: command.into(), args, ..Default::default() }
    }
}
