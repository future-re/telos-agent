use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tools::api::{Tool, ToolContext, ToolDefinition, ToolOutput};
use crate::tools::browser::manager::BrowserManager;
use crate::tools::browser::util::*;

pub struct BrowserNavigateTool {
    manager: BrowserManager,
}

impl BrowserNavigateTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserNavigateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserNavigate".into(),
            description:
                "Navigate a browser session to an http/https URL and return a page summary.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "browser_session_id": { "type": "string" },
                    "headless": { "type": "boolean" },
                    "width": { "type": "integer" },
                    "height": { "type": "integer" },
                    "allowed_domains": { "type": "array", "items": { "type": "string" } },
                    "prohibited_domains": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["url"]
            }),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        let url = arguments
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::Validation("missing string `url`".into()))?;
        validate_http_url(url)
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let url = arguments.get("url").and_then(Value::as_str).unwrap();
        let (_, session, _) = self.manager.start_or_update(&arguments, &context).await?;
        let mut session = session.lock().await;
        let result = session.navigate(url, &context).await?;
        Ok(ToolOutput::json(result))
    }
}
