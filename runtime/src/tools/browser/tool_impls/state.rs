use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
use crate::tools::browser::manager::{BrowserManager, SharedSession};
use crate::tools::browser::util::*;

pub struct BrowserStateTool {
    manager: BrowserManager,
}

impl BrowserStateTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserStateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserState".into(),
            description: "Return visible page text, scroll state, and indexed interactive elements for the browser session.".into(),
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
        let session = self.require_session(&key).await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.state(&context).await?))
    }
}

impl BrowserStateTool {
    async fn require_session(&self, key: &str) -> Result<SharedSession, AgentError> {
        self.manager.get(key).await.ok_or_else(|| AgentError::ToolExecution {
            tool: "BrowserState".into(),
            message: "no browser session found; call BrowserNavigate or BrowserStart first".into(),
        })
    }
}
