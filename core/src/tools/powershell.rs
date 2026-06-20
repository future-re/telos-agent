//! `PowerShell` tool — run PowerShell commands in the workspace cwd.

use async_trait::async_trait;
use base64::Engine;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{optional_usize_any, required_string};

pub struct PowerShellTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerShellEdition {
    Core,
    Desktop,
}

impl PowerShellEdition {
    pub fn from_path(path: &str) -> Self {
        let base = path.rsplit(['/', '\\']).next().unwrap_or(path).to_ascii_lowercase();
        if base.trim_end_matches(".exe") == "pwsh" { Self::Core } else { Self::Desktop }
    }
}

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
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let _command = required_string(&arguments, "command")?;
        let _timeout_ms = optional_usize_any(&arguments, &["timeout_ms"]).unwrap_or(120_000);
        Err(AgentError::ToolExecution {
            tool: "PowerShell".into(),
            message: "PowerShell execution is not implemented yet".into(),
        })
    }
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
