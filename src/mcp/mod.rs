//! MCP (Model Context Protocol) client -- stdio transport.
//!
//! Provides a self-implemented JSON-RPC 2.0 client that spawns an MCP server
//! as a child process and communicates over stdin/stdout.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use tiny_agent_core::mcp::{McpClient, McpServerConfig};
//!
//! # async fn example() -> Result<(), tiny_agent_core::AgentError> {
//! let config = McpServerConfig::new("npx", vec![
//!     "-y".into(),
//!     "@modelcontextprotocol/server-filesystem".into(),
//!     "/some/path".into(),
//! ]);
//! let client = McpClient::new(config);
//! client.connect().await?;
//! for tool in client.tools() {
//!     println!("  {} -- {}", tool.name, tool.description);
//! }
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod config;
pub use client::{McpClient, McpTool};
pub use config::McpServerConfig;
