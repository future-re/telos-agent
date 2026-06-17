use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::error::AgentError;
use crate::mcp::config::McpServerConfig;

/// Monotonically increasing JSON-RPC request ID.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A tool definition returned by an MCP server during the `tools/list` handshake.
#[derive(Debug, Clone)]
pub struct McpTool {
    /// The tool's unique name (used as `name` in `tools/call`).
    pub name: String,
    /// A human-readable description.
    pub description: String,
    /// The JSON Schema for the tool's arguments.
    pub input_schema: Value,
}

/// A connected MCP server via stdio transport.
///
/// Spawns the configured command as a child process, performs the MCP
/// `initialize` handshake, fetches the tool list, and provides a
/// `call_tool` method to invoke server-side tools.
///
/// # Thread safety
///
/// All mutable state is behind `Mutex`, making `&self` the only
/// shared reference needed for every operation. The struct is `Sync`.
pub struct McpClient {
    config: McpServerConfig,
    process: Mutex<Option<Child>>,
    stdin: Mutex<Option<std::process::ChildStdin>>,
    reader: Mutex<Option<BufReader<std::process::ChildStdout>>>,
    server_info: Mutex<Option<Value>>,
    tools: Mutex<Vec<McpTool>>,
}

impl McpClient {
    /// Create a new client for the given server configuration.
    ///
    /// The server is not spawned until [`connect`](Self::connect) is called.
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            process: Mutex::new(None),
            stdin: Mutex::new(None),
            reader: Mutex::new(None),
            server_info: Mutex::new(None),
            tools: Mutex::new(Vec::new()),
        }
    }

    /// Spawn the server process and perform the MCP initialize handshake.
    ///
    /// After the handshake succeeds the client sends the `notifications/initialized`
    /// notification and fetches the tool list via `tools/list`.
    pub async fn connect(&self) -> Result<(), AgentError> {
        let mut cmd = Command::new(&self.config.command);
        cmd.args(&self.config.args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        for (k, v) in &self.config.env {
            cmd.env(k, v);
        }
        if let Some(cwd) = &self.config.cwd {
            cmd.current_dir(cwd);
        }

        let mut child = cmd.spawn().map_err(|e| AgentError::ToolExecution {
            tool: "McpClient".into(),
            message: format!("failed to spawn MCP server '{}': {e}", self.config.command),
        })?;

        let stdin = child.stdin.take().ok_or_else(|| AgentError::ToolExecution {
            tool: "McpClient".into(),
            message: "failed to open child stdin".into(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| AgentError::ToolExecution {
            tool: "McpClient".into(),
            message: "failed to open child stdout".into(),
        })?;

        *self.stdin.lock().unwrap() = Some(stdin);
        *self.reader.lock().unwrap() = Some(BufReader::new(stdout));
        *self.process.lock().unwrap() = Some(child);

        // MCP initialize handshake
        let init_response = self
            .send_request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "tiny-agent-core", "version": "0.1.0" },
                }),
            )
            .await?;

        self.server_info.lock().unwrap().replace(init_response);

        // Send initialized notification
        self.send_notification("notifications/initialized", json!({})).await?;

        // Fetch tools
        let tools_response = self.send_request("tools/list", json!({})).await?;
        *self.tools.lock().unwrap() = Self::parse_tools(&tools_response);

        Ok(())
    }

    /// Return the cached tool list.
    pub fn tools(&self) -> Vec<McpTool> {
        self.tools.lock().unwrap().clone()
    }

    /// Call an MCP tool by name with the given arguments.
    ///
    /// Returns the `result` portion of the JSON-RPC response on success.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, AgentError> {
        self.send_request("tools/call", json!({ "name": name, "arguments": arguments })).await
    }

    /// Kill the server process and release I/O handles.
    pub fn disconnect(&self) {
        if let Some(mut child) = self.process.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        *self.stdin.lock().unwrap() = None;
        *self.reader.lock().unwrap() = None;
    }

    // ── internal helpers ────────────────────────────────────────────

    /// Send a JSON-RPC request and read the matching response.
    async fn send_request(&self, method: &str, params: Value) -> Result<Value, AgentError> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let request_str = serde_json::to_string(&request).unwrap();

        // Write request to child stdin
        if let Some(ref mut stdin) = *self.stdin.lock().unwrap() {
            writeln!(stdin, "{request_str}").map_err(|e| AgentError::ToolExecution {
                tool: "McpClient".into(),
                message: format!("write to MCP server stdin failed: {e}"),
            })?;
            stdin.flush().ok();
        } else {
            return Err(AgentError::ToolExecution {
                tool: "McpClient".into(),
                message: "MCP client not connected (no stdin)".into(),
            });
        }

        // Read one response line from child stdout
        if let Some(ref mut reader) = *self.reader.lock().unwrap() {
            let mut line = String::new();
            reader.read_line(&mut line).map_err(|e| AgentError::ToolExecution {
                tool: "McpClient".into(),
                message: format!("read from MCP server stdout failed: {e}"),
            })?;

            if line.trim().is_empty() {
                return Err(AgentError::ToolExecution {
                    tool: "McpClient".into(),
                    message: "MCP server returned empty response".into(),
                });
            }

            let response: Value =
                serde_json::from_str(&line).map_err(|e| AgentError::ToolExecution {
                    tool: "McpClient".into(),
                    message: format!("parse MCP server response failed: {e}"),
                })?;

            if let Some(err) = response.get("error") {
                return Err(AgentError::ToolExecution {
                    tool: "McpClient".into(),
                    message: format!(
                        "MCP error (code {}): {}",
                        err.get("code").and_then(|v| v.as_i64()).unwrap_or(0),
                        err.get("message").and_then(|v| v.as_str()).unwrap_or("unknown"),
                    ),
                });
            }

            Ok(response.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(AgentError::ToolExecution {
                tool: "McpClient".into(),
                message: "MCP client not connected (no reader)".into(),
            })
        }
    }

    /// Send a JSON-RPC notification (no `id` field, no response expected).
    async fn send_notification(&self, method: &str, params: Value) -> Result<(), AgentError> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let notif_str = serde_json::to_string(&notification).unwrap();

        if let Some(ref mut stdin) = *self.stdin.lock().unwrap() {
            writeln!(stdin, "{notif_str}").ok();
            stdin.flush().ok();
        }
        Ok(())
    }

    /// Parse a `tools/list` response into a `Vec<McpTool>`.
    fn parse_tools(response: &Value) -> Vec<McpTool> {
        response
            .get("tools")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|t| McpTool {
                        name: t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        description: t
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        input_schema: t
                            .get("inputSchema")
                            .cloned()
                            .unwrap_or(json!({"type": "object"})),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a minimal MCP echo-script to a temp file and verify the client
    /// can connect, list tools, and call a tool.
    #[test]
    fn mcp_client_connect_and_list_tools() {
        // Create a temporary shell script that acts as a MCP server.
        let dir = std::env::temp_dir().join(format!("mcp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let script_path = dir.join("echo-mcp.sh");
        let script_content = r#"#!/bin/bash
# Minimal MCP echo server — reads one request line, emits two responses.
read REQUEST_LINE
echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"echo","version":"1.0"},"capabilities":{"tools":{}}}}'
read REQUEST_LINE
echo '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"Echo tool","inputSchema":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}}]}}'
# Keep reading and echoing tool calls
while read REQUEST_LINE; do
  echo '{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"pong"}]}}'
done
"#;
        std::fs::write(&script_path, script_content).unwrap();

        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let config = McpServerConfig::new(script_path.to_str().unwrap(), vec![]);
            let client = McpClient::new(config);

            // connect
            client.connect().await.expect("MCP connect should succeed");

            // list tools
            let tools = client.tools();
            assert_eq!(tools.len(), 1, "expected 1 tool");
            assert_eq!(tools[0].name, "echo");
            assert_eq!(tools[0].description, "Echo tool");

            // call tool
            let result = client
                .call_tool("echo", json!({"text": "hello"}))
                .await
                .expect("call_tool should succeed");
            assert_eq!(result["content"][0]["text"], "pong");

            client.disconnect();
        });

        let _ = std::fs::remove_dir_all(&dir);
    }
}
