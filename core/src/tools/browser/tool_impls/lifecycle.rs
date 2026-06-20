use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
use crate::tools::browser::manager::BrowserManager;
use crate::tools::browser::util::*;
use crate::tools::display_relative;

pub struct BrowserStartTool {
    manager: BrowserManager,
}

impl BrowserStartTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserStartTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserStart".into(),
            description: "Start or reuse an isolated managed Chromium browser session.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "browser_session_id": { "type": "string" },
                    "headless": { "type": "boolean", "default": true },
                    "width": { "type": "integer", "default": 1280 },
                    "height": { "type": "integer", "default": 900 },
                    "allowed_domains": { "type": "array", "items": { "type": "string" } },
                    "prohibited_domains": { "type": "array", "items": { "type": "string" } },
                    "no_sandbox": { "type": "boolean", "description": "Only set when Chromium cannot run in the current sandbox." }
                }
            }),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use BrowserStart when a task needs a dynamic page or full browser automation. \
The managed browser uses an isolated profile by default. Prefer allowed_domains for scoped tasks. \
Do not use browser automation to bypass CAPTCHA, bot checks, paywalls, or access controls.",
        )
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let (key, session, started) = self.manager.start_or_update(&arguments, &context).await?;
        let session = session.lock().await;
        Ok(ToolOutput::json(json!({
            "browser_session_id": key,
            "started": started,
            "port": session.port,
            "artifact_dir": display_relative(&context.cwd, &session.artifact_dir),
            "headless": optional_bool(&arguments, "headless").unwrap_or(true),
            "viewport": { "width": session.viewport.width, "height": session.viewport.height },
            "allowed_domains": session.allowed_domains,
            "prohibited_domains": session.prohibited_domains
        })))
    }
}
pub struct BrowserCloseTool {
    manager: BrowserManager,
}

impl BrowserCloseTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserCloseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserClose".into(),
            description: "Close a managed browser session.".into(),
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
        let Some(session) = self.manager.remove(&key).await else {
            return Ok(ToolOutput::json(json!({
                "browser_session_id": key,
                "closed": false,
                "reason": "session not found"
            })));
        };
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.close().await?))
    }
}
