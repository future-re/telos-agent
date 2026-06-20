use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
use crate::tools::browser::manager::BrowserManager;
use crate::tools::browser::tool_impls::require_session;
use crate::tools::browser::util::*;

pub struct BrowserScrollTool {
    manager: BrowserManager,
}

impl BrowserScrollTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserScrollTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserScroll".into(),
            description: "Scroll the browser page by pixel deltas.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "browser_session_id": { "type": "string" },
                    "delta_x": { "type": "integer", "default": 0 },
                    "delta_y": { "type": "integer", "default": 600 }
                }
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserScroll").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.scroll(&arguments, &context).await?))
    }
}

pub struct BrowserBackTool {
    manager: BrowserManager,
}

impl BrowserBackTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserBackTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserBack".into(),
            description: "Go back in the browser session history and return a page summary.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "browser_session_id": { "type": "string" } }
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserBack").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.back(&context).await?))
    }
}

pub struct BrowserScreenshotTool {
    manager: BrowserManager,
}

impl BrowserScreenshotTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserScreenshot".into(),
            description:
                "Capture a PNG screenshot of the browser page and save it as a workspace artifact."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": { "browser_session_id": { "type": "string" } }
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserScreenshot").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.screenshot(&context).await?))
    }
}
