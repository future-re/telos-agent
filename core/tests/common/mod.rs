#![allow(dead_code)]

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use telos_agent::*;

pub struct AddTool;

#[async_trait]
impl Tool for AddTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "add".into(),
            description: "Add two integers".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "a": { "type": "integer" },
                    "b": { "type": "integer" }
                },
                "required": ["a", "b"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["legacy_add"]
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let a = arguments["a"].as_i64().ok_or_else(|| AgentError::ToolExecution {
            tool: "add".into(),
            message: "missing integer `a`".into(),
        })?;
        let b = arguments["b"].as_i64().ok_or_else(|| AgentError::ToolExecution {
            tool: "add".into(),
            message: "missing integer `b`".into(),
        })?;

        Ok(ToolOutput { content: json!({ "sum": a + b }) })
    }
}

pub struct HangingStreamProvider {
    pub polled: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl ModelProvider for HangingStreamProvider {
    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, AgentError> {
        unreachable!("streaming test provider should not call complete")
    }

    fn stream_complete<'a>(
        &'a self,
        _request: CompletionRequest,
    ) -> std::pin::Pin<
        Box<dyn futures_core::stream::Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>,
    > {
        let polled = Arc::clone(&self.polled);
        Box::pin(futures_util::stream::once(async move {
            polled.notify_waiters();
            std::future::pending::<Result<ProviderEvent, AgentError>>().await
        }))
    }
}

pub struct DenyTool;

#[async_trait]
impl Tool for DenyTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "deny".into(),
            description: "Always deny".into(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Deny { reason: "policy blocked".into() })
    }

    async fn invoke(
        &self,
        _arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        Err(AgentError::ToolExecution { tool: "deny".into(), message: "should not run".into() })
    }
}

pub struct EchoStopHook;

#[async_trait]
impl Hook for EchoStopHook {
    fn name(&self) -> &str {
        "echo-stop"
    }

    fn phase(&self) -> HookPhase {
        HookPhase::Stop
    }

    async fn run(
        &self,
        _context: &HookContext,
        _message: &Message,
    ) -> Result<Option<Message>, AgentError> {
        Ok(Some(Message::assistant("hook-ran")))
    }
}

pub struct BigTool;

#[async_trait]
impl Tool for BigTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "big".into(),
            description: "Return a large result".into(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn invoke(
        &self,
        _arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        Ok(ToolOutput { content: json!({ "blob": "x".repeat(100) }) })
    }
}

pub struct ProgressTool;

#[async_trait]
impl Tool for ProgressTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "progress".into(),
            description: "Emit progress before completing".into(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn invoke(
        &self,
        _arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        if let Some(tx) = &context.progress {
            let _ = tx.send(telos_agent::ToolProgress {
                tool_call_id: context.tool_call_id.clone(),
                message: "halfway".into(),
                data: None,
            });
        }
        Ok(ToolOutput::json(json!({ "done": true })))
    }
}

pub struct WaitTool {
    pub started: Arc<tokio::sync::Notify>,
    pub release: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl Tool for WaitTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "wait".into(),
            description: "Wait until the test releases the tool".into(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn invoke(
        &self,
        _arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        self.started.notify_waiters();
        self.release.notified().await;
        Ok(ToolOutput::json(json!({ "status": "released" })))
    }
}
