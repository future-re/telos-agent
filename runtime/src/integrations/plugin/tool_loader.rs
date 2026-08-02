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
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command as TokioCommand;

use crate::error::AgentError;
use crate::integrations::plugin::PluginError;
use crate::tools::api::{
    InterruptBehavior, PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput,
};

const MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

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
        let remaining = MAX_TOOL_OUTPUT_BYTES.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&chunk[..retained]);
        truncated |= retained < read;
    }
    Ok(LimitedOutput { bytes, truncated })
}

/// Declarative JSON spec for a plugin tool (e.g. `tools/my-tool.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
pub fn load_tool_spec(path: &Path) -> Result<ToolSpec, PluginError> {
    let content = std::fs::read_to_string(path).map_err(|e| PluginError::ManifestParse {
        path: path.to_path_buf(),
        reason: format!("failed to read tool spec: {e}"),
    })?;
    let spec: ToolSpec =
        serde_json::from_str(&content).map_err(|e| PluginError::ManifestParse {
            path: path.to_path_buf(),
            reason: format!("invalid JSON: {e}"),
        })?;
    let mut errors = Vec::new();
    if spec.name.trim().is_empty() {
        errors.push("tool name must not be empty".into());
    }
    if spec.command.trim().is_empty() {
        errors.push("tool command must not be empty".into());
    }
    if spec.timeout_ms == 0 {
        errors.push("tool timeoutMs must be greater than zero".into());
    }
    if !spec.input_schema.is_object() {
        errors.push("tool inputSchema must be a JSON object".into());
    }
    for key in spec.env.keys() {
        if key.is_empty() || key.contains('=') || key.contains('\0') {
            errors.push(format!("tool environment key `{key}` is invalid"));
        }
    }
    if !errors.is_empty() {
        return Err(PluginError::ManifestValidation { errors });
    }
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
    inherit_context_env: bool,
}

impl CommandTool {
    /// Build a CommandTool from a parsed ToolSpec.
    ///
    /// `plugin_root` is prepended to relative command paths and substituted
    /// for `${PLUGIN_ROOT}` in args and env values.
    pub fn from_spec(spec: ToolSpec, plugin_root: &Path) -> Self {
        Self::from_spec_with_env(spec, plugin_root, HashMap::new())
    }

    pub fn from_spec_with_env(
        spec: ToolSpec,
        plugin_root: &Path,
        mut plugin_env: HashMap<String, String>,
    ) -> Self {
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
        plugin_env.extend(env);

        let command = resolve_plugin_command(
            &spec.command.replace("${PLUGIN_ROOT}", &plugin_root_str),
            plugin_root,
        );

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
            env: plugin_env,
            timeout: std::time::Duration::from_millis(spec.timeout_ms),
            is_concurrency_safe: spec.is_concurrency_safe,
            default_permission,
            inherit_context_env: false,
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
        Self {
            definition,
            command,
            args,
            env,
            timeout,
            is_concurrency_safe,
            default_permission,
            inherit_context_env: true,
        }
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
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let args_json = serde_json::to_vec(&arguments)
            .map_err(|e| AgentError::Validation(format!("failed to serialize arguments: {e}")))?;

        let mut command = TokioCommand::new(&self.command);
        command
            .args(&self.args)
            .current_dir(&context.cwd)
            .env_clear()
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if self.inherit_context_env {
            command.envs(context.env.iter());
        }
        command.envs(&self.env);
        hide_console_window(&mut command);
        let mut child = command.spawn().map_err(|e| AgentError::ToolExecution {
            tool: self.definition.name.clone(),
            message: format!("failed to spawn command '{}': {e}", self.command),
        })?;

        // Write JSON arguments to stdin
        let mut stdin = child.stdin.take().ok_or_else(|| AgentError::ToolExecution {
            tool: self.definition.name.clone(),
            message: "failed to open stdin".into(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| AgentError::ToolExecution {
            tool: self.definition.name.clone(),
            message: "failed to open stdout".into(),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| AgentError::ToolExecution {
            tool: self.definition.name.clone(),
            message: "failed to open stderr".into(),
        })?;

        let output = tokio::time::timeout(self.timeout, async {
            match stdin.write_all(&args_json).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
                Err(e) => return Err(e),
            }
            drop(stdin);
            let (status, stdout, stderr) =
                tokio::try_join!(child.wait(), read_limited(stdout), read_limited(stderr))?;
            Ok::<_, std::io::Error>((status, stdout, stderr))
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

        let (status, stdout, stderr) = output;
        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr.bytes);
            return Err(AgentError::ToolExecution {
                tool: self.definition.name.clone(),
                message: format!("tool exited with status {status}: {}", stderr.trim()),
            });
        }
        if stdout.truncated {
            return Err(AgentError::ToolExecution {
                tool: self.definition.name.clone(),
                message: format!("tool output exceeded {MAX_TOOL_OUTPUT_BYTES} bytes"),
            });
        }

        let value: Value =
            serde_json::from_slice(&stdout.bytes).map_err(|e| AgentError::ToolExecution {
                tool: self.definition.name.clone(),
                message: format!("invalid JSON output: {e}"),
            })?;

        Ok(ToolOutput::json(value))
    }
}

fn resolve_plugin_command(command: &str, plugin_root: &Path) -> String {
    let path = Path::new(command);
    if path.is_absolute() || is_bare_executable(command) {
        return command.to_string();
    }
    plugin_root.join(path).to_string_lossy().into_owned()
}

fn is_bare_executable(command: &str) -> bool {
    !command.starts_with('.') && !command.contains('/') && !command.contains('\\')
}

#[cfg_attr(not(windows), allow(unused_variables))]
fn hide_console_window(command: &mut TokioCommand) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[cfg(windows)]
    fn powershell_test_executable() -> String {
        for candidate in ["pwsh", "powershell"] {
            if std::process::Command::new(candidate)
                .args(["-NoProfile", "-NonInteractive", "-Command", "$PSVersionTable.PSVersion"])
                .output()
                .is_ok()
            {
                return candidate.into();
            }
        }
        "powershell".into()
    }

    fn json_echo_command() -> (String, Vec<String>) {
        #[cfg(windows)]
        {
            (
                powershell_test_executable(),
                vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    "[Console]::Out.Write([Console]::In.ReadToEnd())".into(),
                ],
            )
        }
        #[cfg(not(windows))]
        {
            ("cat".into(), vec![])
        }
    }

    fn env_probe_command() -> (String, Vec<String>) {
        #[cfg(windows)]
        {
            (
                powershell_test_executable(),
                vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    concat!(
                        "$null = [Console]::In.ReadToEnd(); ",
                        "$secret = if ($null -eq $env:TELOS_PLUGIN_SECRET) { '' } else { $env:TELOS_PLUGIN_SECRET }; ",
                        "$context = if ($null -eq $env:TELOS_CONTEXT_VISIBLE) { '' } else { $env:TELOS_CONTEXT_VISIBLE }; ",
                        "$configured = if ($null -eq $env:PLUGIN_VISIBLE) { '' } else { $env:PLUGIN_VISIBLE }; ",
                        "[Console]::Out.Write((@{secret=$secret; context=$context; configured=$configured} | ConvertTo-Json -Compress))"
                    )
                    .into(),
                ],
            )
        }
        #[cfg(not(windows))]
        {
            (
                "/bin/sh".into(),
                vec![
                    "-c".into(),
                    "cat >/dev/null; printf '{\"secret\":\"%s\",\"context\":\"%s\",\"configured\":\"%s\"}' \"$TELOS_PLUGIN_SECRET\" \"$TELOS_CONTEXT_VISIBLE\" \"$PLUGIN_VISIBLE\"".into(),
                ],
            )
        }
    }

    fn cwd_probe_command() -> (String, Vec<String>) {
        #[cfg(windows)]
        {
            (
                powershell_test_executable(),
                vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    "$null = [Console]::In.ReadToEnd(); [Console]::Out.Write((@{cwd=(Get-Location).Path} | ConvertTo-Json -Compress))".into(),
                ],
            )
        }
        #[cfg(not(windows))]
        {
            (
                "/bin/sh".into(),
                vec!["-c".into(), "cat >/dev/null; printf '{\"cwd\":\"%s\"}' \"$PWD\"".into()],
            )
        }
    }

    fn failing_command() -> (String, Vec<String>) {
        #[cfg(windows)]
        {
            (
                powershell_test_executable(),
                vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    "exit 1".into(),
                ],
            )
        }
        #[cfg(not(windows))]
        {
            ("/bin/sh".into(), vec!["-c".into(), "exit 1".into()])
        }
    }

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

    #[test]
    fn command_tool_from_spec_resolves_relative_plugin_command() {
        let temp = TempDir::new().unwrap();
        let relative_command = if cfg!(windows) { r"bin\tool.exe" } else { "bin/tool" };
        let spec = ToolSpec {
            name: "test".into(),
            description: "test".into(),
            input_schema: json!({}),
            command: relative_command.into(),
            args: vec![],
            env: HashMap::new(),
            timeout_ms: 5000,
            is_concurrency_safe: false,
            permission: ToolPermission::Ask,
        };

        let tool = CommandTool::from_spec(spec, temp.path());

        assert_eq!(
            std::path::PathBuf::from(&tool.command),
            temp.path().join("bin").join(if cfg!(windows) { "tool.exe" } else { "tool" })
        );
    }

    #[tokio::test]
    async fn command_tool_invoke_echo() {
        let definition = ToolDefinition {
            name: "echo_test".into(),
            description: "Echo test".into(),
            input_schema: json!({"type": "object"}),
        };
        let (command, args) = json_echo_command();

        let tool = CommandTool::new(
            definition,
            command,
            args,
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
        let (command, args) = failing_command();

        let tool = CommandTool::new(
            definition,
            command,
            args,
            HashMap::new(),
            std::time::Duration::from_secs(5),
            false,
            PermissionDecision::Allow,
        );

        let result = tool.invoke(json!({}), ToolContext::dummy()).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("tool exited with status"), "{err}");
    }

    #[tokio::test]
    async fn command_tool_runs_in_context_cwd() {
        let temp = TempDir::new().unwrap();
        let cwd = temp.path().join("workspace");
        std::fs::create_dir(&cwd).unwrap();
        let definition = ToolDefinition {
            name: "cwd_test".into(),
            description: "Cwd test".into(),
            input_schema: json!({"type": "object"}),
        };
        let (command, args) = cwd_probe_command();

        let tool = CommandTool::new(
            definition,
            command,
            args,
            HashMap::new(),
            std::time::Duration::from_secs(5),
            true,
            PermissionDecision::Allow,
        );
        let mut context = ToolContext::dummy();
        context.cwd = cwd.clone();

        let result = tool.invoke(json!({}), context).await.unwrap();

        assert_eq!(
            std::fs::canonicalize(result.content["cwd"].as_str().unwrap()).unwrap(),
            std::fs::canonicalize(&cwd).unwrap()
        );
    }

    #[tokio::test]
    async fn command_tool_runs_with_clean_environment() {
        unsafe { std::env::set_var("TELOS_PLUGIN_SECRET", "leaked") };
        let definition = ToolDefinition {
            name: "env_test".into(),
            description: "Env test".into(),
            input_schema: json!({"type": "object"}),
        };
        let (command, args) = env_probe_command();

        let tool = CommandTool::new(
            definition,
            command,
            args,
            HashMap::from([("PLUGIN_VISIBLE".into(), "yes".into())]),
            std::time::Duration::from_secs(5),
            true,
            PermissionDecision::Allow,
        );
        let mut context = ToolContext::dummy();
        context.env.insert("TELOS_CONTEXT_VISIBLE".into(), "context".into());

        let result = tool.invoke(json!({}), context).await.unwrap();
        unsafe { std::env::remove_var("TELOS_PLUGIN_SECRET") };

        assert_eq!(result.content["secret"], "");
        assert_eq!(result.content["context"], "context");
        assert_eq!(result.content["configured"], "yes");
    }

    #[tokio::test]
    async fn plugin_tool_does_not_inherit_agent_context_environment() {
        let (command, args) = env_probe_command();
        let tool = CommandTool::from_spec_with_env(
            ToolSpec {
                name: "isolated_plugin_tool".into(),
                description: "environment isolation test".into(),
                input_schema: json!({"type": "object"}),
                command,
                args,
                env: HashMap::new(),
                timeout_ms: 5_000,
                is_concurrency_safe: true,
                permission: ToolPermission::Allow,
            },
            Path::new("."),
            HashMap::from([("PLUGIN_VISIBLE".into(), "configured".into())]),
        );
        let mut context = ToolContext::dummy();
        context.env.insert("TELOS_CONTEXT_VISIBLE".into(), "must-not-leak".into());

        let result = tool.invoke(json!({}), context).await.unwrap();

        assert_eq!(result.content["context"], "");
        assert_eq!(result.content["configured"], "configured");
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
