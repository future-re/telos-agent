use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::{display_relative, optional_usize_any, required_string};

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_OUTPUT_CHARS: usize = 20_000;

pub struct PythonScriptTool;

#[async_trait]
impl Tool for PythonScriptTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "PythonScript".into(),
            description:
                "Run a Python script with Telos' isolated bundled Python runtime. Optional packages install into .telos/python/packages, never into the system Python."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "script": { "type": "string" },
                    "packages": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional pip package specifiers to install into the workspace-local Telos Python package directory before running."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Maximum runtime in milliseconds. Defaults to 120000."
                    },
                    "artifacts": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Workspace-relative output files expected from the script."
                    }
                },
                "required": ["script"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["python_script", "python"]
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use PythonScript for data processing, office documents, web parsing, and Playwright automation when Python libraries are helpful. \
Keep scripts self-contained, write outputs under the workspace, and list expected output files in `artifacts`. \
Only request `packages` when the bundled runtime does not already provide the dependency.",
        )
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        required_string(arguments, "script")?;
        package_specs(arguments)?;
        string_array(arguments, "artifacts")?;
        Ok(())
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        let packages = package_specs(arguments)?;
        let reason = if packages.is_empty() {
            "Python script execution requires approval".to_string()
        } else {
            format!(
                "Python script execution requests workspace-local package installation: {}",
                packages.join(", ")
            )
        };
        Ok(PermissionDecision::Ask { reason })
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let runtime = PythonRuntime::resolve(&context)?;
        let script = required_string(&arguments, "script")?;
        let packages = package_specs(&arguments)?;
        let artifacts = string_array(&arguments, "artifacts")?.unwrap_or_default();
        let timeout_ms = optional_usize_any(&arguments, &["timeout_ms"])
            .unwrap_or(DEFAULT_TIMEOUT_MS as usize) as u64;

        let run_dir = context
            .cwd
            .join(".telos")
            .join("python")
            .join("runs")
            .join(format!("{}-{}", context.session_id, context.turn_id));
        tokio::fs::create_dir_all(&run_dir).await.map_err(python_io_error)?;
        let script_path = run_dir.join("script.py");
        tokio::fs::write(&script_path, script).await.map_err(python_io_error)?;

        let package_dir = context.cwd.join(".telos").join("python").join("packages");
        if !packages.is_empty() {
            tokio::fs::create_dir_all(&package_dir).await.map_err(python_io_error)?;
            emit_progress(
                &context,
                "installing Python packages into workspace",
                json!({ "packages": packages.clone(), "target": display_relative(&context.cwd, &package_dir) }),
            );
            let install_output =
                runtime.run_pip_install(&context, &packages, &package_dir, timeout_ms).await?;
            if !install_output.status.success() {
                return Err(AgentError::ToolExecution {
                    tool: "PythonScript".into(),
                    message: format_process_failure("pip install", &install_output),
                });
            }
        }

        emit_progress(
            &context,
            "running Python script",
            json!({
                "python": runtime.python_exe,
                "script": display_relative(&context.cwd, &script_path),
                "timeout_ms": timeout_ms
            }),
        );
        let output = runtime.run_script(&context, &script_path, &package_dir, timeout_ms).await?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(AgentError::ToolExecution {
                tool: "PythonScript".into(),
                message: format_process_failure("python script", &output),
            });
        }

        Ok(ToolOutput::json(json!({
            "success": true,
            "status": output.status.code(),
            "python": runtime.python_exe,
            "script_path": script_path,
            "script_relative_path": display_relative(&context.cwd, &script_path),
            "stdout": trim_large_output(&stdout),
            "stderr": trim_large_output(&stderr),
            "artifacts": collect_artifacts(&context.cwd, &artifacts).await?,
        })))
    }
}

#[derive(Debug, Clone)]
struct PythonRuntime {
    python_exe: PathBuf,
    python_home: Option<PathBuf>,
    playwright_browsers: Option<PathBuf>,
}

impl PythonRuntime {
    fn resolve(context: &ToolContext) -> Result<Self, AgentError> {
        if let Some(path) = context.env.get("TELOS_PYTHON_EXE").filter(|value| !value.is_empty()) {
            let python_exe = PathBuf::from(path);
            if python_exe.exists() {
                return Ok(Self {
                    python_exe,
                    python_home: env_path(&context.env, "TELOS_PYTHON_HOME"),
                    playwright_browsers: env_path(&context.env, "PLAYWRIGHT_BROWSERS_PATH")
                        .or_else(|| env_path(&context.env, "TELOS_PLAYWRIGHT_BROWSERS_PATH")),
                });
            }
        }

        for root in runtime_roots(context) {
            if let Some(runtime) = Self::from_root(&root) {
                return Ok(runtime);
            }
        }

        Err(AgentError::Config(
            "bundled Python runtime not found. Set TELOS_PYTHON_EXE or bundle resources/runtimes/python-<platform>/.".into(),
        ))
    }

    fn from_root(root: &Path) -> Option<Self> {
        let platform = runtime_platform();
        let candidates = [
            root.join("runtimes").join(format!("python-{platform}")),
            root.join("runtimes").join("python").join(platform),
            root.join("python"),
        ];
        for python_home in candidates {
            let python_exe = python_home.join(python_executable_name());
            if python_exe.exists() {
                let playwright_browsers =
                    [root.join("playwright-browsers"), python_home.join("playwright-browsers")]
                        .into_iter()
                        .find(|path| path.exists());
                return Some(Self { python_exe, python_home: None, playwright_browsers });
            }
        }
        None
    }

    async fn run_pip_install(
        &self,
        context: &ToolContext,
        packages: &[String],
        package_dir: &Path,
        timeout_ms: u64,
    ) -> Result<std::process::Output, AgentError> {
        let mut command = self.base_command(context, Some(package_dir));
        command
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--disable-pip-version-check")
            .arg("--target")
            .arg(package_dir)
            .args(packages)
            .current_dir(&context.cwd);
        run_child("PythonScript", command, timeout_ms).await
    }

    async fn run_script(
        &self,
        context: &ToolContext,
        script_path: &Path,
        package_dir: &Path,
        timeout_ms: u64,
    ) -> Result<std::process::Output, AgentError> {
        let mut command = self.base_command(context, Some(package_dir));
        command.arg(script_path).current_dir(&context.cwd);
        run_child("PythonScript", command, timeout_ms).await
    }

    fn base_command(&self, context: &ToolContext, package_dir: Option<&Path>) -> Command {
        let mut command = Command::new(&self.python_exe);
        command.env_clear().envs(context.env.iter());
        command.env("PYTHONNOUSERSITE", "1");
        command.env("PIP_DISABLE_PIP_VERSION_CHECK", "1");
        if let Some(home) = &self.python_home {
            command.env("PYTHONHOME", home);
        }
        if let Some(path) = &self.playwright_browsers {
            command.env("PLAYWRIGHT_BROWSERS_PATH", path);
        }
        if let Some(package_dir) = package_dir {
            let mut pythonpath = package_dir.as_os_str().to_os_string();
            if let Some(existing) = context.env.get("PYTHONPATH").filter(|value| !value.is_empty())
            {
                pythonpath.push(if cfg!(windows) { ";" } else { ":" });
                pythonpath.push(existing);
            }
            command.env("PYTHONPATH", pythonpath);
        }
        hide_console_window(&mut command);
        command.kill_on_drop(true);
        command
    }
}

fn runtime_roots(context: &ToolContext) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = env_path(&context.env, "TELOS_RUNTIME_DIR") {
        roots.push(path);
    }
    roots.push(context.cwd.join("desktop").join("src-tauri").join("resources"));
    roots.push(context.cwd.join("resources"));
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        roots.push(parent.join("resources"));
        roots.push(parent.join("../Resources").join("resources"));
        roots.push(parent.join("../../Resources").join("resources"));
    }
    roots
}

fn env_path(env: &std::collections::HashMap<String, String>, key: &str) -> Option<PathBuf> {
    env.get(key).filter(|value| !value.trim().is_empty()).map(PathBuf::from)
}

fn runtime_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("macos", "x86_64") => "macos-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        _ => "unknown",
    }
}

fn python_executable_name() -> &'static str {
    if cfg!(windows) { "python.exe" } else { "bin/python3" }
}

async fn run_child(
    tool: &str,
    mut command: Command,
    timeout_ms: u64,
) -> Result<std::process::Output, AgentError> {
    command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| AgentError::ToolExecution { tool: tool.into(), message: err.to_string() })?;
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

    let status = timeout(Duration::from_millis(timeout_ms.max(1)), child.wait())
        .await
        .map_err(|_| AgentError::ToolExecution {
            tool: tool.into(),
            message: format!("Python command timed out after {timeout_ms}ms"),
        })?
        .map_err(|err| AgentError::ToolExecution { tool: tool.into(), message: err.to_string() })?;
    let stdout = stdout_task
        .await
        .map_err(|err| AgentError::ToolExecution { tool: tool.into(), message: err.to_string() })?
        .map_err(|err| AgentError::ToolExecution { tool: tool.into(), message: err.to_string() })?;
    let stderr = stderr_task
        .await
        .map_err(|err| AgentError::ToolExecution { tool: tool.into(), message: err.to_string() })?
        .map_err(|err| AgentError::ToolExecution { tool: tool.into(), message: err.to_string() })?;
    Ok(std::process::Output { status, stdout, stderr })
}

async fn collect_artifacts(cwd: &Path, artifacts: &[String]) -> Result<Vec<Value>, AgentError> {
    let mut values = Vec::new();
    for artifact in artifacts {
        let path = super::resolve_workspace_path(cwd, artifact)?;
        let path = super::canonicalize_within_cwd(cwd, &path).await?;
        let metadata = tokio::fs::metadata(&path).await.map_err(python_io_error)?;
        values.push(json!({
            "path": path,
            "relative_path": display_relative(cwd, &path),
            "bytes": metadata.len(),
        }));
    }
    Ok(values)
}

fn package_specs(arguments: &Value) -> Result<Vec<String>, AgentError> {
    let Some(packages) = string_array(arguments, "packages")? else {
        return Ok(Vec::new());
    };
    for package in &packages {
        if package.trim().is_empty() || package.contains(['\0', '\n', '\r']) {
            return Err(AgentError::Validation("invalid Python package specifier".into()));
        }
    }
    Ok(packages)
}

fn string_array(arguments: &Value, key: &str) -> Result<Option<Vec<String>>, AgentError> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    let Some(array) = value.as_array() else {
        return Err(AgentError::Validation(format!("`{key}` must be an array of strings")));
    };
    let mut values = Vec::with_capacity(array.len());
    for item in array {
        let Some(value) = item.as_str() else {
            return Err(AgentError::Validation(format!("`{key}` must be an array of strings")));
        };
        values.push(value.to_string());
    }
    Ok(Some(values))
}

fn emit_progress(context: &ToolContext, message: &str, data: Value) {
    if let Some(tx) = &context.progress {
        let _ = tx.send(crate::tool::ToolProgress {
            tool_call_id: context.tool_call_id.clone(),
            message: message.into(),
            data: Some(data),
        });
    }
}

fn format_process_failure(action: &str, output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!(
        "{action} failed with exit code {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        trim_large_output(&stdout),
        trim_large_output(&stderr)
    )
}

fn trim_large_output(output: &str) -> String {
    if output.chars().count() <= MAX_OUTPUT_CHARS {
        return output.to_string();
    }
    let preview = output.chars().take(MAX_OUTPUT_CHARS).collect::<String>();
    format!("{preview}\n<truncated output after {MAX_OUTPUT_CHARS} chars>")
}

fn python_io_error(err: std::io::Error) -> AgentError {
    AgentError::ToolExecution { tool: "PythonScript".into(), message: err.to_string() }
}

#[cfg_attr(not(windows), allow(unused_variables))]
fn hide_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn package_specs_rejects_control_characters() {
        let result = package_specs(&json!({ "packages": ["ok", "bad\npkg"] }));
        assert!(result.is_err());
    }

    #[test]
    fn runtime_platform_is_known_on_supported_targets() {
        assert_ne!(runtime_platform(), "unknown");
    }
}
