pub mod app;
pub mod approval;
pub mod chat_panel;
pub mod event;
pub mod input_panel;
pub mod markdown;
pub mod status_bar;
pub mod theme;

use anyhow::Result;
use std::sync::Arc;
use telos_agent::{AgentConfig, ModelProvider, ToolRegistry};

/// Launch the ratatui full-screen TUI.
pub async fn run(
    _config: AgentConfig,
    _provider: Arc<dyn ModelProvider>,
    _tools: ToolRegistry,
    _status_text: String,
) -> Result<()> {
    todo!("TUI entry point implemented in Task 4")
}
