//! Plugin hook loading — converts plugin hook definitions into Hook implementations.

use async_trait::async_trait;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::agent::hooks::{Hook, HookContext, HookPhase};
use crate::error::AgentError;
use crate::model::message::Message;

/// A hook that executes an external command when triggered.
///
/// The hook serialises the `HookContext` and the current `Message` as JSON
/// and pipes them to the command's stdin. If the command writes content to
/// stdout, that content is returned as a follow-up assistant message.
pub struct CommandHook {
    name: String,
    command: String,
    args: Vec<String>,
    phase: HookPhase,
    timeout_ms: u64,
}

impl CommandHook {
    pub fn new(
        name: String,
        command: String,
        args: Vec<String>,
        phase: HookPhase,
        timeout_ms: u64,
    ) -> Self {
        Self { name, command, args, phase, timeout_ms }
    }
}

#[async_trait]
impl Hook for CommandHook {
    fn name(&self) -> &str {
        &self.name
    }

    fn phase(&self) -> HookPhase {
        self.phase.clone()
    }

    async fn run(
        &self,
        context: &HookContext,
        message: &Message,
    ) -> Result<Option<Message>, AgentError> {
        let input = json!({
            "session_id": context.session_id,
            "turn_id": context.turn_id,
            "message_count": context.message_count,
            "message": message,
        });

        let input_str = serde_json::to_string(&input).map_err(|e| AgentError::ToolExecution {
            tool: self.name.clone(),
            message: format!("hook serialization error: {e}"),
        })?;

        let mut command = Command::new(&self.command);
        command
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        hide_console_window(&mut command);
        let mut child = command.spawn().map_err(|e| AgentError::ToolExecution {
            tool: self.name.clone(),
            message: format!("hook command failed to spawn: {e}"),
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            let mut all = input_str.into_bytes();
            all.push(b'\n');
            stdin.write_all(&all).await.map_err(|e| AgentError::ToolExecution {
                tool: self.name.clone(),
                message: format!("hook stdin write failed: {e}"),
            })?;
        }

        let output = tokio::time::timeout(
            std::time::Duration::from_millis(self.timeout_ms),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| AgentError::ToolExecution {
            tool: self.name.clone(),
            message: "hook command timed out".into(),
        })?
        .map_err(|e| AgentError::ToolExecution {
            tool: self.name.clone(),
            message: format!("hook command failed: {e}"),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() {
                tracing::warn!(
                    hook = %self.name,
                    stderr = %stderr,
                    "hook command produced no stdout but wrote to stderr"
                );
            }
            return Ok(None);
        }

        Ok(Some(Message::assistant(stdout)))
    }
}

#[cfg_attr(not(windows), allow(unused_variables))]
fn hide_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}
