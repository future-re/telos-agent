use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::runtime::{AgentSession, TurnEvent};
use crate::subagent::AgentDefinition;
use crate::subagent::tool::SubagentTool;
use crate::tool::{ToolContext, ToolOutput, ToolProgress, ToolRegistry};

impl SubagentTool {
    pub(super) async fn run_agent(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
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
            let mut stream =
                Box::pin(session.run_turn_stream(&self.provider, &child_tools, prompt.to_string()));
            while let Some(event) = stream.next().await {
                let event = event?;
                if let Some((progress, data)) = progress_summary(&event, &agent_id, &agent.name)
                    && let Some(tx) = &context.progress
                {
                    let _ = tx.send(ToolProgress {
                        tool_call_id: context.tool_call_id.clone(),
                        message: progress,
                        data: Some(data),
                    });
                }
                events.push(event);
            }
        }

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

pub(super) fn filter_tools_for_agent(
    tools: &ToolRegistry,
    agent: &AgentDefinition,
) -> ToolRegistry {
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
