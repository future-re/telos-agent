//! Command-backed plugin policy with a controlled working directory and environment.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use crate::agent::policies::{Policy, PolicyContext, PolicyOutcome};
use crate::error::AgentError;

const MAX_POLICY_OUTPUT_BYTES: usize = 1024 * 1024;

struct LimitedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn read_limited(mut reader: impl AsyncRead + Unpin) -> std::io::Result<LimitedOutput> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_POLICY_OUTPUT_BYTES.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&chunk[..retained]);
        truncated |= retained < read;
    }
    Ok(LimitedOutput { bytes, truncated })
}

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
        let stdout = child.stdout.take().expect("policy stdout must be piped");
        let stderr = child.stderr.take().expect("policy stderr must be piped");
        let output =
            tokio::time::timeout(std::time::Duration::from_millis(self.timeout_ms), async {
                let (status, stdout, stderr) =
                    tokio::try_join!(child.wait(), read_limited(stdout), read_limited(stderr))?;
                Ok::<_, std::io::Error>((status, stdout, stderr))
            })
            .await
            .map_err(|_| AgentError::ToolExecution {
                tool: self.name.clone(),
                message: "policy timed out".into(),
            })?
            .map_err(|error| AgentError::ToolExecution {
                tool: self.name.clone(),
                message: format!("policy failed: {error}"),
            })?;
        let (status, stdout, stderr) = output;
        if !status.success() {
            let stderr_text = String::from_utf8_lossy(&stderr.bytes).trim().to_string();
            let detail = if stderr_text.is_empty() {
                format!("policy exited with status {status}")
            } else if stderr.truncated {
                format!("{stderr_text}… [truncated]")
            } else {
                stderr_text
            };
            return Err(AgentError::ToolExecution { tool: self.name.clone(), message: detail });
        }
        if stdout.truncated {
            return Err(AgentError::ToolExecution {
                tool: self.name.clone(),
                message: format!("policy output exceeded {MAX_POLICY_OUTPUT_BYTES} bytes"),
            });
        }
        serde_json::from_slice(&stdout.bytes).map_err(|error| AgentError::ToolExecution {
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

    #[cfg(unix)]
    #[tokio::test]
    async fn executes_command_policy_with_json_protocol() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("check-policy.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
payload=$(cat)
case "$payload" in
  *'"point":"session_start"'*) ;;
  *) echo "missing policy context" >&2; exit 2 ;;
esac
printf '%s' '{"decision":"continue","feedback":["checked"]}'
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&script, permissions).unwrap();

        let policy = CommandPolicy::new(
            "test-policy".into(),
            "./check-policy.sh".into(),
            Vec::new(),
            1_000,
            dir.path().to_path_buf(),
            HashMap::new(),
        );
        let outcome = policy
            .evaluate(&PolicyContext::SessionStart {
                session_id: "session-1".into(),
                mode: crate::SessionMode::Create,
                message_count: 0,
            })
            .await
            .unwrap();

        assert_eq!(
            outcome,
            PolicyOutcome {
                decision: crate::PolicyDecision::Continue,
                feedback: vec!["checked".into()],
            }
        );
    }
}
