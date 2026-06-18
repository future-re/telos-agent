//! Subagent module — in-process nested agents and the Fork concurrent-execution engine.

pub mod fork;
pub use fork::{ForkExecution, ForkLens, ForkResult, ForkShared, Synapse};

// In-process subagent tool — runs a nested agent session as a tool call.
// Useful when the parent agent wants to delegate a self-contained sub-task
// to an isolated agent that has its own turn loop, its own message history,
// and its own iteration cap. The subagent shares the parent's tool registry
// and model provider but starts with a fresh conversation.

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

/// Tool that delegates to a nested agent session or runs fork lenses.
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

    /// Execute a fork run: run each lens through the provider concurrently.
    async fn run_fork(
        &self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let forks = arguments
            .get("forks")
            .and_then(|f| f.as_array())
            .ok_or_else(|| AgentError::Validation("fork mode requires `forks` array".into()))?;

        let lenses: Vec<ForkLens> = forks
            .iter()
            .filter_map(|item| {
                let lens = item.get("lens")?.as_str()?;
                let system_prompt = item.get("system_prompt")?.as_str()?;
                let task = item.get("task")?.as_str()?;
                Some(ForkLens {
                    lens: lens.to_string(),
                    system_prompt: system_prompt.to_string(),
                    task: task.to_string(),
                    output_schema: item.get("output_schema").cloned(),
                    allowed_tools: item
                        .get("allowed_tools")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                })
            })
            .collect();

        if lenses.is_empty() {
            return Err(AgentError::Validation(
                "fork mode requires at least one lens with `lens`, `system_prompt`, and `task`"
                    .into(),
            ));
        }

        let fork_shared = ForkShared {
            provider: self.provider.clone(),
            tool_registry: self.tools.clone(),
            messages: context.messages.clone(),
            config: self.config.clone(),
        };

        let synapse = Synapse::new(4); // reasonable default concurrency
        let execution = synapse.run_all(&fork_shared, lenses, None).await;

        let results: Vec<Value> = execution
            .results
            .iter()
            .map(|r| match r {
                Some(ForkResult::Text(text)) => json!({ "text": text }),
                Some(ForkResult::Structured(val)) => {
                    json!({ "structured": val, "text": val.to_string() })
                }
                None => json!({ "error": "lens execution failed" }),
            })
            .collect();

        Ok(ToolOutput::json(json!({
            "mode": "fork",
            "lens_count": results.len(),
            "results": results,
        })))
    }
}

#[async_trait]
impl Tool for SubagentTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "subagent".into(),
            description:
                "Run an in-process subagent with its own turn loop or execute fork lenses concurrently."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string" },
                    "system_prompt": { "type": "string" },
                    "max_iterations": { "type": "integer" },
                    "mode": {
                        "type": "string",
                        "enum": ["agent", "fork"],
                        "description": "When 'fork', runs multiple concurrent lenses instead of a full agent session"
                    },
                    "forks": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "lens": { "type": "string" },
                                "system_prompt": { "type": "string" },
                                "task": { "type": "string" },
                                "output_schema": { "type": "object" },
                                "allowed_tools": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                }
                            },
                            "required": ["lens", "system_prompt", "task"]
                        }
                    }
                },
                "required": ["prompt"]
            }),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use the Subagent tool to delegate self-contained tasks, run parallel explore lenses, or protect the main context window. \
Provide a clear prompt and optional system_prompt. Do not duplicate work already being performed in the parent session.",
        )
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        arguments
            .get("prompt")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AgentError::Validation("missing string `prompt`".into()))?;

        let mode = arguments.get("mode").and_then(|v| v.as_str()).unwrap_or("agent");

        if mode == "fork" {
            let forks = arguments
                .get("forks")
                .and_then(|f| f.as_array())
                .ok_or_else(|| AgentError::Validation("fork mode requires `forks` array".into()))?;
            if forks.is_empty() {
                return Err(AgentError::Validation(
                    "fork mode requires at least one fork entry".into(),
                ));
            }
            for (i, item) in forks.iter().enumerate() {
                if item.get("lens").and_then(|v| v.as_str()).is_none() {
                    return Err(AgentError::Validation(format!("fork[{}] missing `lens`", i)));
                }
                if item.get("system_prompt").and_then(|v| v.as_str()).is_none() {
                    return Err(AgentError::Validation(format!(
                        "fork[{}] missing `system_prompt`",
                        i
                    )));
                }
                if item.get("task").and_then(|v| v.as_str()).is_none() {
                    return Err(AgentError::Validation(format!("fork[{}] missing `task`", i)));
                }
            }
        }

        Ok(())
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Ask { reason: "subagent execution requires approval".into() })
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let mode = arguments.get("mode").and_then(|v| v.as_str()).unwrap_or("agent");

        match mode {
            "fork" => {
                let result = self.run_fork(&arguments, &context).await?;
                Ok(result)
            }
            _ => {
                // Default agent mode
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
                if let Some(system_prompt) =
                    arguments.get("system_prompt").and_then(|value| value.as_str())
                {
                    config.base_system_prompt = Some(system_prompt.to_string());
                }
                if let Some(max_iterations) =
                    arguments.get("max_iterations").and_then(|value| value.as_u64())
                {
                    config.max_iterations = max_iterations.max(1) as usize;
                }

                let mut session = AgentSession::new(config)?;
                let mut events = Vec::new();
                {
                    let mut stream = Box::pin(session.run_turn_stream(
                        &self.provider,
                        &self.tools,
                        prompt.to_string(),
                    ));
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
