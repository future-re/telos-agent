use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::error::AgentError;
use crate::mcp::client::McpTool;
use crate::mcp::manager::McpManager;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

/// Wraps an MCP server tool as a native [`Tool`].
///
/// Each bridge holds a reference to the [`McpManager`] so that `invoke` can
/// dispatch the call to the correct server.
pub struct McpToolBridge {
    server_id: String,
    mcp_tool: McpTool,
    manager: Arc<McpManager>,
}

impl McpToolBridge {
    /// Create a new bridge wrapping an MCP tool on a named server.
    pub fn new(server_id: String, mcp_tool: McpTool, manager: Arc<McpManager>) -> Self {
        Self { server_id, mcp_tool, manager }
    }

    /// Normalized tool name: `"mcp__<server>__<tool>"`.
    ///
    /// This namespace avoids collisions when multiple MCP servers export tools
    /// with the same short name.
    pub fn normalized_name(server_id: &str, tool_name: &str) -> String {
        format!("mcp__{server_id}__{tool_name}")
    }
}

#[async_trait]
impl Tool for McpToolBridge {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::normalized_name(&self.server_id, &self.mcp_tool.name),
            description: format!("[MCP:{}] {}", self.server_id, self.mcp_tool.description),
            input_schema: self.mcp_tool.input_schema.clone(),
        }
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        false
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Ask {
            reason: format!("MCP tool '{}' from server '{}'", self.mcp_tool.name, self.server_id),
        })
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let result =
            self.manager.call_tool(&self.server_id, &self.mcp_tool.name, arguments).await?;
        Ok(ToolOutput::json(result))
    }
}
