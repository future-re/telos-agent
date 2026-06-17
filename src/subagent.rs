//! In-process subagent tool — runs a nested [`AgentSession`] as a tool call.
//!
//! Useful when the parent agent wants to delegate a self-contained sub-task
//! (e.g. "summarise these files") to an isolated agent that has its own turn
//! loop, its own message history, and its own iteration cap. The subagent
//! shares the parent's tool registry and model provider but starts with a
//! fresh conversation.

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::provider::ModelProvider;
use crate::runtime::{AgentSession, TurnEvent};
use crate::tool::{
    PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry,
};

/// Tool that delegates to a nested agent session.
pub struct SubagentTool {
    provider: Arc<dyn ModelProvider + Send + Sync>,
    tools: ToolRegistry,
    config: AgentConfig,
}

impl SubagentTool {
    pub fn new(
        provider: Arc<dyn ModelProvider + Send + Sync>,
        tools: ToolRegistry,
        config: AgentConfig,
    ) -> Self {
        Self { provider, tools, config }
    }
}

#[async_trait]
impl Tool for SubagentTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "subagent".into(),
            description:
                "Run an in-process subagent with its own turn loop and return its final answer."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string" },
                    "system_prompt": { "type": "string" },
                    "max_iterations": { "type": "integer" }
                },
                "required": ["prompt"]
            }),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        arguments
            .get("prompt")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AgentError::Validation("missing string `prompt`".into()))?;
        Ok(())
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Ask {
            reason: "subagent execution requires approval".into(),
        })
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let prompt = arguments
            .get("prompt")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AgentError::Validation("missing string `prompt`".into()))?;

        // Clone the parent config and override the runtime-specific bits.
        // Storage is disabled because the subagent is ephemeral; permissions
        // are forwarded so the nested run still honours global rules.
        let mut config = self.config.clone();
        config.cwd = context.cwd;
        config.env = context.env;
        config.storage = None;
        if let Some(system_prompt) = arguments.get("system_prompt").and_then(|value| value.as_str())
        {
            config.system_prompt = Some(system_prompt.to_string());
        }
        if let Some(max_iterations) =
            arguments.get("max_iterations").and_then(|value| value.as_u64())
        {
            config.max_iterations = max_iterations.max(1) as usize;
        }

        let mut session = AgentSession::new(config);
        let mut events = Vec::new();
        {
            let mut stream =
                Box::pin(session.run_turn_stream(&self.provider, &self.tools, prompt.to_string()));
            while let Some(event) = stream.next().await {
                let event = event?;
                // Forward a coarse-grained progress message to the parent's
                // tool-progress channel so callers can show "subagent is doing X".
                if let Some(progress) = progress_summary(&event)
                    && let Some(tx) = &context.progress
                {
                    let _ = tx.send(crate::tool::ToolProgress {
                        tool_call_id: None,
                        message: progress,
                        data: None,
                    });
                }
                events.push(event);
            }
        }

        // The subagent's "answer" is the most recent assistant message.
        let final_text = session
            .messages()
            .iter()
            .rev()
            .find(|message| message.role == crate::message::Role::Assistant)
            .map(|message| message.text_content())
            .unwrap_or_default();

        Ok(ToolOutput::json(json!({
            "session_id": session.session_id(),
            "final_text": final_text,
            "event_count": events.len(),
        })))
    }
}

/// Translate a subset of subagent events into human-readable progress strings
/// for the parent agent to surface.
fn progress_summary(event: &TurnEvent) -> Option<String> {
    match event {
        TurnEvent::ProviderRequest { iteration, .. } => {
            Some(format!("subagent provider request iteration {iteration}"))
        }
        TurnEvent::ToolCall { name, .. } => Some(format!("subagent tool call {name}")),
        TurnEvent::TurnFinished { stop_reason, .. } => {
            Some(format!("subagent finished with {stop_reason:?}"))
        }
        _ => None,
    }
}
