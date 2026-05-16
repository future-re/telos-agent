use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::error::AgentError;
use crate::message::Message;

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

    pub fn json(content: Value) -> Self {
        Self { content }
    }
}

#[derive(Debug, Clone)]
pub struct ToolProgress {
    pub tool_call_id: Option<String>,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptBehavior {
    Block,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny { reason: String },
    Ask { reason: String },
}

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub session_id: String,
    pub turn_id: u64,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub messages: Vec<Message>,
    pub progress: Option<mpsc::UnboundedSender<ToolProgress>>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    async fn validate(&self, _arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        Ok(())
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Allow)
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Block
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        false
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError>;
}

#[derive(Default, Clone)]
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

    pub fn get(&self, name: &str) -> Result<Arc<dyn Tool>, AgentError> {
        self.tools
            .get(name)
            .cloned()
            .ok_or_else(|| AgentError::ToolNotFound(name.to_string()))
    }
}
