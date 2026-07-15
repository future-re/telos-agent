use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};
use crate::tools::browser::manager::BrowserManager;
use crate::tools::browser::tool_impls::require_session;
use crate::tools::browser::util::*;

pub struct BrowserClickTool {
    manager: BrowserManager,
}

impl BrowserClickTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserClickTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserClick".into(),
            description:
                "Click a browser element. Prefer element_id from BrowserState. The selector field is CSS, with text=... and xpath=... accepted as locator shorthands."
                    .into(),
            input_schema: selector_schema(json!({})),
        }
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        sensitive_action_permission("browser click", arguments)
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserClick").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.click(&arguments, &context).await?))
    }
}

pub struct BrowserTypeTool {
    manager: BrowserManager,
}

impl BrowserTypeTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserTypeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserType".into(),
            description:
                "Type text into a browser input, textarea, or contenteditable element. Prefer element_id from BrowserState. The selector field is CSS, with text=... and xpath=... accepted as locator shorthands."
                    .into(),
            input_schema: selector_schema(json!({
                "text": { "type": "string" },
                "clear": { "type": "boolean", "default": true }
            })),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        if arguments.get("text").and_then(Value::as_str).is_none() {
            return Err(AgentError::Validation("missing string `text`".into()));
        }
        Ok(())
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        sensitive_action_permission("browser typing", arguments)
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserType").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.type_text(&arguments, &context).await?))
    }
}

pub struct BrowserSelectTool {
    manager: BrowserManager,
}

impl BrowserSelectTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserSelectTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserSelect".into(),
            description:
                "Select a value in a browser select element. Prefer element_id from BrowserState. The selector field is CSS, with text=... and xpath=... accepted as locator shorthands."
                    .into(),
            input_schema: selector_schema(json!({
                "value": { "type": "string" }
            })),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        if arguments.get("value").and_then(Value::as_str).is_none() {
            return Err(AgentError::Validation("missing string `value`".into()));
        }
        Ok(())
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        sensitive_action_permission("browser select", arguments)
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserSelect").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.select(&arguments, &context).await?))
    }
}
