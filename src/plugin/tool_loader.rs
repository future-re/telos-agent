//! CommandTool — declarative JSON-defined tools executed as subprocesses.
//!
//! Plugin tools are defined as JSON files specifying a command, optional args,
//! timeout, and permission level. At runtime, arguments are piped as JSON to
//! stdin; stdout JSON is the tool result.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

use crate::error::AgentError;
use crate::tool::{
    InterruptBehavior, PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput,
};

/// Declarative JSON spec for a plugin tool (e.g. `tools/my-tool.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    #[serde(default = "default_tool_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub is_concurrency_safe: bool,
    /// Default permission decision when no rule matches.
    #[serde(default = "default_permission")]
    pub permission: ToolPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPermission {
    Allow,
    Ask,
    Deny,
}

fn default_tool_timeout_ms() -> u64 {
    60_000
}

fn default_permission() -> ToolPermission {
    ToolPermission::Ask
}

/// Load a tool spec from a JSON file.
pub fn load_tool_spec(path: &Path) -> Result<ToolSpec, AgentError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        AgentError::Config(format!("failed to read tool spec {}: {e}", path.display()))
    })?;
    let spec: ToolSpec = serde_json::from_str(&content)
        .map_err(|e| AgentError::Config(format!("invalid tool spec {}: {e}", path.display())))?;
    Ok(spec)
}

/// A `Tool` implementation backed by a subprocess command.
///
/// Arguments are serialized to JSON and piped to the command's stdin.
/// The command must write a JSON value to stdout before exiting.
pub struct CommandTool {
    definition: ToolDefinition,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    timeout: std::time::Duration,
    is_concurrency_safe: bool,
    default_permission: PermissionDecision,
}

impl CommandTool {
    /// Build a CommandTool from a parsed ToolSpec.
    ///
    /// `plugin_root` is prepended to relative command paths and substituted
    /// for `${PLUGIN_ROOT}` in args and env values.
    pub fn from_spec(spec: ToolSpec, plugin_root: &Path) -> Self {
        let plugin_root_str = plugin_root.to_string_lossy();

        // Substitute ${PLUGIN_ROOT} in args
        let args: Vec<String> =
            spec.args.into_iter().map(|a| a.replace("${PLUGIN_ROOT}", &plugin_root_str)).collect();

        // Substitute ${PLUGIN_ROOT} in env values
        let env: HashMap<String, String> = spec
            .env
            .into_iter()
            .map(|(k, v)| (k, v.replace("${PLUGIN_ROOT}", &plugin_root_str)))
            .collect();

        let command = spec.command.replace("${PLUGIN_ROOT}", &plugin_root_str);

        let definition = ToolDefinition {
            name: spec.name,
            description: spec.description,
            input_schema: spec.input_schema,
        };

        let default_permission = match spec.permission {
            ToolPermission::Allow => PermissionDecision::Allow,
            ToolPermission::Ask => {
                PermissionDecision::Ask { reason: "plugin tool requires approval".into() }
            }
            ToolPermission::Deny => PermissionDecision::Deny {
                reason: "plugin tool is configured to deny by default".into(),
            },
        };

        Self {
            definition,
            command,
            args,
            env,
            timeout: std::time::Duration::from_millis(spec.timeout_ms),
            is_concurrency_safe: spec.is_concurrency_safe,
            default_permission,
        }
    }

    /// Create a CommandTool directly (for programmatic construction).
    pub fn new(
        definition: ToolDefinition,
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
        timeout: std::time::Duration,
        is_concurrency_safe: bool,
        default_permission: PermissionDecision,
    ) -> Self {
        Self { definition, command, args, env, timeout, is_concurrency_safe, default_permission }
    }
}

#[async_trait]
impl Tool for CommandTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        self.is_concurrency_safe
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Cancel
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(self.default_permission.clone())
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let args_json = serde_json::to_vec(&arguments)
            .map_err(|e| AgentError::Validation(format!("failed to serialize arguments: {e}")))?;

        let mut child = TokioCommand::new(&self.command)
            .args(&self.args)
            .envs(&self.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AgentError::ToolExecution {
                tool: self.definition.name.clone(),
                message: format!("failed to spawn command '{}': {e}", self.command),
            })?;

        // Write JSON arguments to stdin
        let mut stdin = child.stdin.take().ok_or_else(|| AgentError::ToolExecution {
            tool: self.definition.name.clone(),
            message: "failed to open stdin".into(),
        })?;

        let output = tokio::time::timeout(self.timeout, async {
            stdin.write_all(&args_json).await?;
            drop(stdin);
            child.wait_with_output().await
        })
        .await
        .map_err(|_| AgentError::ToolExecution {
            tool: self.definition.name.clone(),
            message: format!("tool timed out after {}ms", self.timeout.as_millis()),
        })?
        .map_err(|e| AgentError::ToolExecution {
            tool: self.definition.name.clone(),
            message: format!("I/O error: {e}"),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AgentError::ToolExecution {
                tool: self.definition.name.clone(),
                message: format!("tool exited with status {}: {}", output.status, stderr.trim()),
            });
        }

        let value: Value =
            serde_json::from_slice(&output.stdout).map_err(|e| AgentError::ToolExecution {
                tool: self.definition.name.clone(),
                message: format!("invalid JSON output: {e}"),
            })?;

        Ok(ToolOutput::json(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn parse_tool_spec_minimal() {
        let json = json!({
            "name": "my_tool",
            "description": "A test",
            "inputSchema": {"type": "object"},
            "command": "echo"
        });
        let spec: ToolSpec = serde_json::from_value(json).unwrap();
        assert_eq!(spec.name, "my_tool");
        assert_eq!(spec.command, "echo");
        assert!(spec.args.is_empty());
        assert_eq!(spec.timeout_ms, 60_000);
        assert!(!spec.is_concurrency_safe);
    }

    #[test]
    fn parse_tool_spec_full() {
        let json = json!({
            "name": "full_tool",
            "description": "Full spec",
            "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}},
            "command": "python3",
            "args": ["-u", "${PLUGIN_ROOT}/scripts/tool.py"],
            "env": {"PYTHONUNBUFFERED": "1"},
            "timeoutMs": 10000,
            "isConcurrencySafe": true,
            "permission": "allow"
        });
        let spec: ToolSpec = serde_json::from_value(json).unwrap();
        assert_eq!(spec.name, "full_tool");
        assert_eq!(spec.args.len(), 2);
        assert_eq!(spec.timeout_ms, 10_000);
        assert!(spec.is_concurrency_safe);
        assert!(matches!(spec.permission, ToolPermission::Allow));
    }

    #[test]
    fn command_tool_from_spec_substitutes_plugin_root() {
        let spec = ToolSpec {
            name: "test".into(),
            description: "test".into(),
            input_schema: json!({}),
            command: "${PLUGIN_ROOT}/bin/tool".into(),
            args: vec!["--config".into(), "${PLUGIN_ROOT}/config.json".into()],
            env: HashMap::from([("TOOL_HOME".into(), "${PLUGIN_ROOT}/home".into())]),
            timeout_ms: 5000,
            is_concurrency_safe: false,
            permission: ToolPermission::Ask,
        };

        let tool = CommandTool::from_spec(spec, Path::new("/opt/plugin"));
        assert_eq!(tool.command, "/opt/plugin/bin/tool");
        assert_eq!(tool.args, vec!["--config", "/opt/plugin/config.json"]);
        assert_eq!(tool.env.get("TOOL_HOME").unwrap(), "/opt/plugin/home");
    }

    #[tokio::test]
    async fn command_tool_invoke_echo() {
        let definition = ToolDefinition {
            name: "echo_test".into(),
            description: "Echo test".into(),
            input_schema: json!({"type": "object"}),
        };

        let tool = CommandTool::new(
            definition,
            "cat".into(),
            vec![],
            HashMap::new(),
            std::time::Duration::from_secs(5),
            true,
            PermissionDecision::Allow,
        );

        let result = tool.invoke(json!({"hello": "world"}), ToolContext::dummy()).await.unwrap();

        let content = result.content;
        assert_eq!(content["hello"], "world");
    }

    #[tokio::test]
    async fn command_tool_invoke_failure() {
        let definition = ToolDefinition {
            name: "fail_test".into(),
            description: "Fail test".into(),
            input_schema: json!({"type": "object"}),
        };

        // Using `false` which exits with code 1
        let tool = CommandTool::new(
            definition,
            "false".into(),
            vec![],
            HashMap::new(),
            std::time::Duration::from_secs(5),
            false,
            PermissionDecision::Allow,
        );

        let result = tool.invoke(json!({}), ToolContext::dummy()).await;
        assert!(result.is_err());
    }

    #[test]
    fn load_tool_spec_from_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tool.json");
        std::fs::write(
            &path,
            serde_json::to_string(&json!({
                "name": "file_tool",
                "description": "Loaded from file",
                "inputSchema": {"type": "object"},
                "command": "echo",
                "permission": "allow"
            }))
            .unwrap(),
        )
        .unwrap();

        let spec = load_tool_spec(&path).unwrap();
        assert_eq!(spec.name, "file_tool");
    }
}
