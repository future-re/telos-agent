//! Subagent module — in-process nested agents and the Fork concurrent-execution engine.

pub mod builtins;
pub mod definition;
pub mod fork;
pub mod registry;
pub use definition::{AgentDefinition, AgentIsolation, AgentSource};
pub use fork::{ForkExecution, ForkLens, ForkResult, ForkShared, Synapse};
pub use registry::SubagentRegistry;

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
    registry: SubagentRegistry,
}

impl SubagentTool {
    pub fn new(
        provider: Arc<dyn ModelProvider + Send + Sync>,
        tools: ToolRegistry,
        config: AgentConfig,
    ) -> Self {
        Self { provider, tools, config, registry: SubagentRegistry::with_builtin_agents() }
    }

    pub fn with_registry(
        provider: Arc<dyn ModelProvider + Send + Sync>,
        tools: ToolRegistry,
        config: AgentConfig,
        registry: SubagentRegistry,
    ) -> Self {
        Self { provider, tools, config, registry }
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
                    "description": {
                        "type": "string",
                        "description": "Short 3-5 word description of what the agent will do"
                    },
                    "prompt": { "type": "string" },
                    "subagent_type": {
                        "type": "string",
                        "description": "Specialized agent type to use; defaults to general-purpose"
                    },
                    "system_prompt": { "type": "string" },
                    "max_iterations": { "type": "integer" },
                    "model": {
                        "type": "string",
                        "enum": ["thinking", "execution", "recovery", "summarization", "inherit"]
                    },
                    "run_in_background": { "type": "boolean" },
                    "isolation": {
                        "type": "string",
                        "enum": ["none", "worktree"]
                    },
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
Provide a clear description and prompt. Use subagent_type to choose a specialized agent when appropriate: general-purpose, Explore, Plan, or Verification. \
Do not duplicate work already being performed in the parent session.",
        )
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        arguments
            .get("prompt")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AgentError::Validation("missing string `prompt`".into()))?;

        if let Some(agent_type) = arguments.get("subagent_type").and_then(|v| v.as_str())
            && self.registry.get(agent_type).is_none()
        {
            let available = self
                .registry
                .definitions()
                .into_iter()
                .map(|agent| agent.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AgentError::Validation(format!(
                "unknown subagent_type `{agent_type}`. Available agents: {available}"
            )));
        }

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
                let agent_type = arguments
                    .get("subagent_type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("general-purpose");
                let agent = self.registry.get(agent_type).ok_or_else(|| {
                    AgentError::Validation(format!("unknown subagent_type `{agent_type}`"))
                })?;
                let description = arguments
                    .get("description")
                    .and_then(|value| value.as_str())
                    .unwrap_or(agent.description.as_str());
                let agent_id = new_agent_id(agent_type);

                // Clone the parent config and override the runtime-specific bits.
                // Storage is disabled because the subagent is ephemeral; permissions
                // are forwarded so the nested run still honours global rules.
                let mut config = self.config.clone();
                config.cwd = context.cwd;
                config.env = context.env;
                config.storage = None;
                config.base_system_prompt = Some(
                    arguments
                        .get("system_prompt")
                        .and_then(|value| value.as_str())
                        .unwrap_or(agent.system_prompt.as_str())
                        .to_string(),
                );
                config.prompt_assembly = None;
                if let Some(max_iterations) = arguments
                    .get("max_iterations")
                    .and_then(|value| value.as_u64())
                    .or(agent.max_iterations.map(|value| value as u64))
                {
                    config.max_iterations = max_iterations.max(1) as usize;
                }

                let mut session = AgentSession::new(config)?;
                let child_tools = filter_tools_for_agent(&self.tools, agent);
                let mut events = Vec::new();
                {
                    let mut stream = Box::pin(session.run_turn_stream(
                        &self.provider,
                        &child_tools,
                        prompt.to_string(),
                    ));
                    while let Some(event) = stream.next().await {
                        let event = event?;
                        // Forward a coarse-grained progress message to the parent's
                        // tool-progress channel so callers can show "subagent is doing X".
                        if let Some((progress, data)) =
                            progress_summary(&event, &agent_id, &agent.name)
                            && let Some(tx) = &context.progress
                        {
                            let _ = tx.send(crate::tool::ToolProgress {
                                tool_call_id: context.tool_call_id.clone(),
                                message: progress,
                                data: Some(data),
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
                    "agent_id": agent_id,
                    "agent_type": agent.name,
                    "description": description,
                    "status": "completed",
                    "session_id": session.session_id(),
                    "final_text": final_text,
                    "event_count": events.len(),
                })))
            }
        }
    }
}

fn filter_tools_for_agent(tools: &ToolRegistry, agent: &AgentDefinition) -> ToolRegistry {
    tools.filtered(&agent.allowed_tools, &agent.disallowed_tools)
}

fn new_agent_id(agent_type: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let safe_type: String = agent_type
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '_' })
        .collect();
    format!("agent_{safe_type}_{:x}", now.as_nanos())
}

/// Translate a subset of subagent events into human-readable progress strings
/// for the parent agent to surface.
fn progress_summary(
    event: &TurnEvent,
    agent_id: &str,
    agent_type: &str,
) -> Option<(String, Value)> {
    match event {
        TurnEvent::ProviderRequest { iteration, .. } => Some((
            format!("subagent provider request iteration {iteration}"),
            json!({
                "kind": "subagent",
                "agent_id": agent_id,
                "agent_type": agent_type,
                "event": "provider_request",
                "iteration": iteration,
            }),
        )),
        TurnEvent::ToolCall { tool_call_id, name, detail } => Some((
            format!("subagent tool call {name}"),
            json!({
                "kind": "subagent",
                "agent_id": agent_id,
                "agent_type": agent_type,
                "event": "tool_call",
                "tool_call_id": tool_call_id,
                "name": name,
                "detail": detail,
            }),
        )),
        TurnEvent::TurnFinished { stop_reason, .. } => Some((
            format!("subagent finished with {stop_reason:?}"),
            json!({
                "kind": "subagent",
                "agent_id": agent_id,
                "agent_type": agent_type,
                "event": "finished",
                "stop_reason": format!("{stop_reason:?}"),
            }),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockProvider;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Arc;

    struct NamedTool(&'static str);

    #[async_trait]
    impl Tool for NamedTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.0.into(),
                description: "test".into(),
                input_schema: json!({"type": "object"}),
            }
        }

        async fn invoke(
            &self,
            _arguments: Value,
            _context: ToolContext,
        ) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }

    #[test]
    fn subagent_tool_schema_exposes_agent_tool_fields() {
        let tool = SubagentTool::new(
            Arc::new(MockProvider::new(vec![])),
            ToolRegistry::new(),
            AgentConfig::default(),
        );
        let schema = tool.definition().input_schema;
        let properties = schema["properties"].as_object().unwrap();
        assert!(properties.contains_key("description"));
        assert!(properties.contains_key("subagent_type"));
        assert!(properties.contains_key("model"));
        assert!(properties.contains_key("run_in_background"));
        assert!(properties.contains_key("isolation"));
    }

    #[test]
    fn subagent_prompt_text_names_supported_agent_types() {
        let tool = SubagentTool::new(
            Arc::new(MockProvider::new(vec![])),
            ToolRegistry::new(),
            AgentConfig::default(),
        );
        let prompt = tool.prompt_text().unwrap();
        assert!(prompt.contains("subagent_type"));
        assert!(prompt.contains("general-purpose"));
        assert!(prompt.contains("Explore"));
        assert!(prompt.contains("Plan"));
        assert!(prompt.contains("Verification"));
    }

    #[tokio::test]
    async fn validate_rejects_unknown_subagent_type_with_available_agents() {
        let tool = SubagentTool::new(
            Arc::new(MockProvider::new(vec![])),
            ToolRegistry::new(),
            AgentConfig::default(),
        );

        let err = tool
            .validate(
                &json!({
                    "prompt": "inspect",
                    "subagent_type": "Nope"
                }),
                &ToolContext::dummy(),
            )
            .await
            .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("unknown subagent_type `Nope`"));
        assert!(message.contains("Explore"));
    }

    #[tokio::test]
    async fn subagent_type_selects_agent_prompt_and_result_metadata() {
        let provider = Arc::new(MockProvider::new(vec![crate::provider::CompletionResponse {
            message: crate::Message::assistant("explore result"),
            stop_reason: crate::provider::StopReason::EndTurn,
            usage: None,
        }]));
        let tool = SubagentTool::new(provider.clone(), ToolRegistry::new(), AgentConfig::default());

        let output = tool
            .invoke(
                json!({
                    "description": "Explore code",
                    "prompt": "Find the runtime loop",
                    "subagent_type": "Explore"
                }),
                ToolContext::dummy(),
            )
            .await
            .unwrap();

        let requests = provider.requests.lock().await;
        let system_prompt = requests[0].system_prompt.as_deref().unwrap_or_default();
        assert!(system_prompt.contains("You are an explore agent"), "{system_prompt}");
        drop(requests);

        assert_eq!(output.content["agent_type"], "Explore");
        assert_eq!(output.content["description"], "Explore code");
        assert_eq!(output.content["status"], "completed");
        assert_eq!(output.content["final_text"], "explore result");
        assert!(output.content["agent_id"].as_str().unwrap().starts_with("agent_"));
    }

    #[tokio::test]
    async fn subagent_progress_is_attached_to_parent_tool_call_with_data() {
        let provider = Arc::new(MockProvider::new(vec![
            crate::provider::CompletionResponse {
                message: crate::Message {
                    role: crate::Role::Assistant,
                    blocks: vec![crate::ContentBlock::ToolCall(crate::ToolCall {
                        id: "child-call".into(),
                        name: "Read".into(),
                        arguments: json!({ "file_path": "src/lib.rs" }),
                    })],
                },
                stop_reason: crate::provider::StopReason::ToolUse,
                usage: None,
            },
            crate::provider::CompletionResponse {
                message: crate::Message::assistant("done"),
                stop_reason: crate::provider::StopReason::EndTurn,
                usage: None,
            },
        ]));
        let tool = SubagentTool::new(provider, ToolRegistry::new(), AgentConfig::default());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut context = ToolContext::dummy();
        context.tool_call_id = Some("parent-call".into());
        context.progress = Some(tx);

        tool.invoke(
            json!({
                "description": "Explore code",
                "prompt": "Find the runtime loop",
                "subagent_type": "Explore"
            }),
            context,
        )
        .await
        .unwrap();

        let mut progress = Vec::new();
        while let Ok(item) = rx.try_recv() {
            progress.push(item);
        }

        assert!(progress.iter().any(|item| {
            item.tool_call_id.as_deref() == Some("parent-call")
                && item.data.as_ref().and_then(|data| data["kind"].as_str()) == Some("subagent")
                && item.data.as_ref().and_then(|data| data["event"].as_str()) == Some("tool_call")
                && item.data.as_ref().and_then(|data| data["name"].as_str()) == Some("Read")
        }));
    }

    #[test]
    fn filters_child_tools_with_agent_allowlist_and_denylist() {
        let mut tools = ToolRegistry::new();
        tools.register(NamedTool("Read"));
        tools.register(NamedTool("Write"));
        tools.register(NamedTool("Grep"));

        let mut agent = AgentDefinition::new("limited", "limited", "prompt", AgentSource::BuiltIn);
        agent.allowed_tools = vec!["Read".into(), "Write".into()];
        agent.disallowed_tools = vec!["Write".into()];

        let filtered = filter_tools_for_agent(&tools, &agent);
        let names = filtered.definitions().into_iter().map(|def| def.name).collect::<Vec<_>>();

        assert_eq!(names, vec!["Read"]);
        assert!(filtered.get("Read").is_ok());
        assert!(filtered.get("Write").is_err());
        assert!(filtered.get("Grep").is_err());
    }

    #[test]
    fn wildcard_allowlist_keeps_all_tools_before_denylist() {
        let mut tools = ToolRegistry::new();
        tools.register(NamedTool("Read"));
        tools.register(NamedTool("Write"));

        let mut agent = AgentDefinition::new("wild", "wild", "prompt", AgentSource::BuiltIn);
        agent.allowed_tools = vec!["*".into()];
        agent.disallowed_tools = vec!["Write".into()];

        let filtered = filter_tools_for_agent(&tools, &agent);

        assert!(filtered.get("Read").is_ok());
        assert!(filtered.get("Write").is_err());
    }
}
