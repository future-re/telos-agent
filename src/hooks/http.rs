use crate::error::AgentError;
use std::collections::HashMap;
use std::process::Command;

/// Execute an HTTP-type hook by spawning curl.
pub fn exec_http_hook(
    url: &str,
    method: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Result<String, AgentError> {
    let mut cmd = Command::new("curl");
    cmd.args(["-s", "-X", method, "--max-time", "30"]);
    for (k, v) in headers {
        cmd.args(["-H", &format!("{k}: {v}")]);
    }
    if let Some(body) = body {
        cmd.args(["-d", body]);
    }
    cmd.arg(url);

    let output = cmd.output().map_err(|e| AgentError::ToolExecution {
        tool: "HttpHook".into(),
        message: e.to_string(),
    })?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
