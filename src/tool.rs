use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::AgentError;

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: Value,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: json!({ "text": text.into() }),
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    async fn invoke(&self, arguments: Value) -> Result<ToolOutput, AgentError>;
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T>(&mut self, tool: T)
    where
        T: Tool + 'static,
    {
        let definition = tool.definition();
        self.tools.insert(definition.name.clone(), Arc::new(tool));
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|tool| tool.definition())
            .collect::<Vec<_>>()
    }

    pub async fn invoke(&self, name: &str, arguments: Value) -> Result<ToolOutput, AgentError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| AgentError::ToolNotFound(name.to_string()))?;

        tool.invoke(arguments).await
    }
}
