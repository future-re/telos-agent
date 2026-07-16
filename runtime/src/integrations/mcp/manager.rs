use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::error::AgentError;
use crate::integrations::mcp::client::{McpClient, McpTool};
use crate::integrations::mcp::config::McpServerConfig;

/// Manages multiple MCP server connections.
pub struct McpManager {
    servers: Mutex<HashMap<String, McpServerHandle>>,
}

struct McpServerHandle {
    config: McpServerConfig,
    client: McpClient,
    connected: bool,
}

impl McpManager {
    /// Create a manager from a set of named server configs.
    pub fn new(servers: HashMap<String, McpServerConfig>) -> Self {
        let handles = servers
            .into_iter()
            .map(|(id, config)| {
                let client = McpClient::new(config.clone());
                (id, McpServerHandle { config, client, connected: false })
            })
            .collect();
        Self { servers: Mutex::new(handles) }
    }

    /// Load from `.tiny-agent/mcp.json`.
    ///
    /// The expected JSON format is:
    /// ```json
    /// { "mcpServers": { "name": { "command": "...", "args": [...], "auto_connect": true } } }
    /// ```
    pub fn load_config(path: &Path) -> Result<Self, AgentError> {
        if !path.exists() {
            return Ok(Self { servers: Mutex::new(HashMap::new()) });
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| AgentError::Config(format!("failed to read MCP config: {e}")))?;
        let config: Value = serde_json::from_str(&content)
            .map_err(|e| AgentError::Config(format!("failed to parse MCP config: {e}")))?;
        let servers_map = config.get("mcpServers").and_then(|v| v.as_object());
        let mut servers = HashMap::new();
        if let Some(map) = servers_map {
            for (name, server_cfg) in map {
                let command = server_cfg.get("command").and_then(|v| v.as_str()).unwrap_or("");
                if command.is_empty() {
                    tracing::warn!(
                        server = %name,
                        "MCP server config missing 'command' field — skipping"
                    );
                    continue;
                }
                let args: Vec<String> = server_cfg
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let auto_connect =
                    server_cfg.get("auto_connect").and_then(|v| v.as_bool()).unwrap_or(true);
                let cfg = McpServerConfig {
                    command: command.to_string(),
                    args,
                    auto_connect,
                    ..McpServerConfig::default()
                };
                servers.insert(name.clone(), cfg);
            }
        }
        Ok(Self::new(servers))
    }

    /// Register a new server configuration. If a server with the same id
    /// already exists, it is replaced.
    pub async fn register_server(&self, id: String, config: McpServerConfig) {
        let mut servers = self.servers.lock().await;
        let client = McpClient::new(config.clone());
        servers.insert(id, McpServerHandle { config, client, connected: false });
    }

    /// Register multiple servers from a map of configs.
    pub async fn register_servers(&self, new_servers: HashMap<String, McpServerConfig>) {
        let mut servers = self.servers.lock().await;
        for (id, config) in new_servers {
            let client = McpClient::new(config.clone());
            servers.insert(id, McpServerHandle { config, client, connected: false });
        }
    }

    /// Connect all servers with `auto_connect` enabled.
    pub async fn connect_all(&self) {
        let mut servers = self.servers.lock().await;
        for (id, handle) in servers.iter_mut() {
            if handle.config.auto_connect && !handle.connected {
                match handle.client.connect().await {
                    Ok(()) => {
                        handle.connected = true;
                        tracing::info!(server = %id, "MCP server connected");
                    }
                    Err(e) => {
                        tracing::warn!(server = %id, error = %e, "MCP server failed to connect");
                    }
                }
            }
        }
    }

    /// Collect all tools from all connected servers.
    ///
    /// Returns `(server_id, tool)` pairs.
    pub async fn all_tools(&self) -> Vec<(String, McpTool)> {
        let servers = self.servers.lock().await;
        servers
            .iter()
            .filter(|(_, h)| h.connected)
            .flat_map(|(id, h)| h.client.tools().into_iter().map(move |t| (id.clone(), t)))
            .collect()
    }

    /// Call a tool on a specific server.
    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        args: Value,
    ) -> Result<Value, AgentError> {
        let servers = self.servers.lock().await;
        let handle = servers.get(server_id).ok_or_else(|| {
            AgentError::ToolNotFound(format!("MCP server '{server_id}' not found"))
        })?;
        handle.client.call_tool(tool_name, args).await
    }

    /// Disconnect all servers.
    pub async fn disconnect_all(&self) {
        let servers = self.servers.lock().await;
        for (_, handle) in servers.iter() {
            handle.client.disconnect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn load_config_file_not_found() {
        let path = PathBuf::from("/tmp/nonexistent-mcp-config.json");
        let manager =
            McpManager::load_config(&path).expect("load_config should succeed for missing file");
        assert!(manager.servers.try_lock().unwrap().is_empty());
    }

    #[test]
    fn load_config_valid_json() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = dir.path().join("mcp.json");
        let json = r#"{
            "mcpServers": {
                "fs": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
                    "auto_connect": true
                },
                "noargs": {
                    "command": "echo",
                    "auto_connect": false
                }
            }
        }"#;
        std::fs::write(&config_path, json).unwrap();

        let manager = McpManager::load_config(&config_path)
            .expect("load_config should succeed for valid JSON");
        let servers = manager.servers.try_lock().unwrap();
        assert_eq!(servers.len(), 2);
        let fs = servers.get("fs").expect("fs server should exist");
        assert_eq!(fs.config.command, "npx");
        assert_eq!(fs.config.args, vec!["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]);
        assert!(fs.config.auto_connect);

        let noargs = servers.get("noargs").expect("noargs server should exist");
        assert_eq!(noargs.config.command, "echo");
        assert!(!noargs.config.auto_connect);
    }

    #[test]
    fn load_config_invalid_json_returns_error() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = dir.path().join("bad.json");
        std::fs::write(&config_path, "not json").unwrap();

        let result = McpManager::load_config(&config_path);
        match result {
            Err(AgentError::Config(_)) => {} // expected
            Err(e) => panic!("expected AgentError::Config, got: {e}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[test]
    fn new_creates_empty_manager() {
        let manager = McpManager::new(HashMap::new());
        let servers = manager.servers.try_lock().unwrap();
        assert!(servers.is_empty());
    }

    #[test]
    fn new_creates_manager_with_servers() {
        let mut servers = HashMap::new();
        servers.insert("test".to_string(), McpServerConfig::new("echo", vec![]));
        let manager = McpManager::new(servers);
        let handles = manager.servers.try_lock().unwrap();
        assert_eq!(handles.len(), 1);
        let handle = handles.get("test").unwrap();
        assert_eq!(handle.config.command, "echo");
        assert!(!handle.connected);
    }
}
