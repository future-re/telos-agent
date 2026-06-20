//! `PowerShell` tool — run PowerShell commands in the workspace cwd.

use async_trait::async_trait;
use base64::Engine;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{optional_usize_any, required_string};

pub struct PowerShellTool;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerShellEdition {
    Core,
    Desktop,
}

impl PowerShellEdition {
    #[allow(dead_code)]
    pub fn from_path(path: &str) -> Self {
        let base = path.rsplit(['/', '\\']).next().unwrap_or(path).to_ascii_lowercase();
        if base.trim_end_matches(".exe") == "pwsh" { Self::Core } else { Self::Desktop }
    }
}

#[allow(dead_code)]
pub fn encode_powershell_command(command: &str) -> String {
    let mut bytes = Vec::with_capacity(command.len() * 2);
    for unit in command.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn build_powershell_args(command: &str) -> Vec<String> {
    vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), command.into()]
}

pub fn find_powershell_executable() -> Option<String> {
    if let Ok(path) = std::env::var("TELOS_POWERSHELL_PATH")
        && !path.trim().is_empty()
    {
        return Some(path);
    }
    let candidates: &[&str] =
        if cfg!(windows) { &["pwsh.exe", "powershell.exe"] } else { &["pwsh", "powershell"] };
    candidates.iter().find(|candidate| executable_exists(candidate)).map(|s| (*s).into())
}

fn executable_exists(candidate: &str) -> bool {
    std::process::Command::new(candidate)
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("$PSVersionTable.PSVersion")
        .output()
        .is_ok()
}

#[async_trait]
impl Tool for PowerShellTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "PowerShell".into(),
            description: "Run a PowerShell command in the current working directory. Prefer Read/Edit/Write/Glob/Grep for file operations.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "description": { "type": "string" },
                    "timeout_ms": { "type": "integer", "description": "Maximum runtime in milliseconds. Defaults to 120000." }
                },
                "required": ["command"]
            }),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use the PowerShell tool for Windows-native shell commands. \
Prefer Read, Edit, Write, Glob, or Grep for file operations. \
Use PowerShell syntax, not Bash syntax. \
Provide a short `description` summarizing the command's intent.",
        )
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string(arguments, "command").map(|_| ())
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        let command = required_string(arguments, "command")?;
        match crate::powershell_security::analyze(command) {
            crate::powershell_security::CommandSafety::Safe => Ok(PermissionDecision::Allow),
            crate::powershell_security::CommandSafety::NeedsReview { reason } => {
                Ok(PermissionDecision::Ask {
                    reason: format!("PowerShell command needs review: {reason}"),
                })
            }
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let command = required_string(&arguments, "command")?;
        let timeout_ms =
            optional_usize_any(&arguments, &["timeout_ms"]).unwrap_or(120_000).max(1) as u64;
        if let Some(tx) = &context.progress {
            let _ = tx.send(crate::tool::ToolProgress {
                tool_call_id: None,
                message: format!("running PowerShell command with {timeout_ms}ms timeout"),
                data: Some(json!({ "command": command, "timeout_ms": timeout_ms })),
            });
        }
        let executable = find_powershell_executable().ok_or_else(|| AgentError::ToolExecution {
            tool: "PowerShell".into(),
            message: "PowerShell executable not found; install pwsh or powershell".into(),
        })?;
        let mut child = Command::new(executable);
        child
            .args(build_powershell_args(command))
            .current_dir(&context.cwd)
            .env_clear()
            .envs(context.env.iter());
        child.kill_on_drop(true);
        let output = timeout(Duration::from_millis(timeout_ms), run_powershell_child(child))
            .await
            .map_err(|_| AgentError::ToolExecution {
                tool: "PowerShell".into(),
                message: format!("Command timed out after {timeout_ms}ms"),
            })?
            .map_err(|err| AgentError::ToolExecution {
                tool: "PowerShell".into(),
                message: err.to_string(),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(AgentError::ToolExecution {
                tool: "PowerShell".into(),
                message: format!(
                    "Command failed with exit code {:?}\nstdout:\n{}\nstderr:\n{}",
                    output.status.code(),
                    trim_large_output(&stdout),
                    trim_large_output(&stderr)
                ),
            });
        }
        Ok(ToolOutput::json(json!({
            "status": output.status.code(),
            "success": output.status.success(),
            "stdout": trim_large_output(&stdout),
            "stderr": trim_large_output(&stderr),
        })))
    }
}

async fn run_powershell_child(mut command: Command) -> std::io::Result<std::process::Output> {
    command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.map(|_| buf)
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map(|_| buf)
    });

    let status = child.wait().await?;
    let stdout = stdout_task.await.map_err(std::io::Error::other)??;
    let stderr = stderr_task.await.map_err(std::io::Error::other)??;
    Ok(std::process::Output { status, stdout, stderr })
}

fn trim_large_output(output: &str) -> String {
    const MAX_CHARS: usize = 20_000;
    if output.chars().count() <= MAX_CHARS {
        return output.to_string();
    }
    let preview = output.chars().take(MAX_CHARS).collect::<String>();
    format!("{preview}\n<truncated output after {MAX_CHARS} chars>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_powershell_edition_from_path() {
        assert_eq!(PowerShellEdition::from_path("pwsh"), PowerShellEdition::Core);
        assert_eq!(
            PowerShellEdition::from_path("C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
            PowerShellEdition::Core
        );
        assert_eq!(PowerShellEdition::from_path("powershell.exe"), PowerShellEdition::Desktop);
    }

    #[test]
    fn encoded_command_uses_utf16le_base64() {
        assert_eq!(
            encode_powershell_command("Write-Output hi"),
            "VwByAGkAdABlAC0ATwB1AHQAcAB1AHQAIABoAGkA"
        );
    }

    #[test]
    fn build_args_use_noninteractive_no_profile_command() {
        assert_eq!(
            build_powershell_args("Get-Process"),
            vec!["-NoProfile", "-NonInteractive", "-Command", "Get-Process"]
        );
    }
}
