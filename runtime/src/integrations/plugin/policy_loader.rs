//! Command-backed plugin policy with a controlled working directory and environment.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::agent::policies::{Policy, PolicyContext, PolicyOutcome};
use crate::error::AgentError;

pub struct CommandPolicy {
    name: String,
    command: String,
    args: Vec<String>,
    timeout_ms: u64,
    plugin_root: PathBuf,
    env: HashMap<String, String>,
}

impl CommandPolicy {
    pub fn new(
        name: String,
        command: String,
        args: Vec<String>,
        timeout_ms: u64,
        plugin_root: PathBuf,
        env: HashMap<String, String>,
    ) -> Self {
        Self { name, command, args, timeout_ms, plugin_root, env }
    }

    fn resolve(&self, value: &str) -> String {
        value.replace("${PLUGIN_ROOT}", &self.plugin_root.to_string_lossy())
    }

    fn command_path(&self) -> String {
        let command = self.resolve(&self.command);
        let path = Path::new(&command);
        if path.is_relative()
            && (command.starts_with('.') || command.contains('/') || command.contains('\\'))
        {
            self.plugin_root.join(path).to_string_lossy().into_owned()
        } else {
            command
        }
    }
}

#[async_trait]
impl Policy for CommandPolicy {
    fn name(&self) -> &str {
        &self.name
    }

    async fn evaluate(&self, context: &PolicyContext) -> Result<PolicyOutcome, AgentError> {
        let input = serde_json::to_vec(context).map_err(|error| AgentError::ToolExecution {
            tool: self.name.clone(),
            message: format!("policy serialization error: {error}"),
        })?;
        let mut command = Command::new(self.command_path());
        command
            .args(self.args.iter().map(|arg| self.resolve(arg)))
            .current_dir(&self.plugin_root)
            .env_clear()
            .envs(&self.env)
            .env("PLUGIN_ROOT", &self.plugin_root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| AgentError::ToolExecution {
            tool: self.name.clone(),
            message: format!("policy failed to spawn: {error}"),
        })?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&input).await.map_err(|error| AgentError::ToolExecution {
                tool: self.name.clone(),
                message: format!("policy stdin failed: {error}"),
            })?;
        }
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(self.timeout_ms),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| AgentError::ToolExecution {
            tool: self.name.clone(),
            message: "policy timed out".into(),
        })?
        .map_err(|error| AgentError::ToolExecution {
            tool: self.name.clone(),
            message: format!("policy failed: {error}"),
        })?;
        if !output.status.success() {
            return Err(AgentError::ToolExecution {
                tool: self.name.clone(),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        serde_json::from_slice(&output.stdout).map_err(|error| AgentError::ToolExecution {
            tool: self.name.clone(),
            message: format!("invalid policy outcome: {error}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_plugin_root_in_commands_and_arguments() {
        let root = PathBuf::from("example-plugin");
        let policy = CommandPolicy::new(
            "p".into(),
            "./bin/check".into(),
            vec!["${PLUGIN_ROOT}/config.json".into()],
            1000,
            root.clone(),
            HashMap::new(),
        );
        assert_eq!(PathBuf::from(policy.command_path()), root.join("./bin/check"));
        assert_eq!(PathBuf::from(policy.resolve(&policy.args[0])), root.join("config.json"));
    }
}
