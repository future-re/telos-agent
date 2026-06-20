use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::config::CancellationState;
use crate::error::AgentError;
use crate::runtime::{AgentSession, TurnEvent};
use crate::subagent::AgentDefinition;
use crate::subagent::tool::SubagentTool;
use crate::tasks::{Task, TaskStatus};
use crate::tool::{ToolContext, ToolOutput, ToolProgress, ToolRegistry};

struct SubagentRunResult {
    session_id: String,
    final_text: String,
    event_count: usize,
}

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

        let system_prompt = arguments
            .get("system_prompt")
            .and_then(|value| value.as_str())
            .unwrap_or(agent.system_prompt.as_str())
            .to_string();
        let max_iterations = arguments
            .get("max_iterations")
            .and_then(|value| value.as_u64())
            .or(agent.max_iterations.map(|value| value as u64))
            .map(|value| value.max(1) as usize);

        if arguments.get("run_in_background").and_then(|value| value.as_bool()).unwrap_or(false) {
            let task_manager = self.config.task_manager.clone().ok_or_else(|| {
                AgentError::Validation("run_in_background requires AgentConfig.task_manager".into())
            })?;
            let agent_type_name = agent.name.clone();
            let cancellation = CancellationState::new();
            let task = Task {
                id: agent_id.clone(),
                subject: description.to_string(),
                description: prompt.to_string(),
                status: TaskStatus::InProgress,
                blocked_by: vec![],
                blocks: vec![],
                output: None,
                kind: Some("subagent".into()),
                agent_id: Some(agent_id.clone()),
                agent_type: Some(agent_type_name.clone()),
                worktree_path: None,
                error: None,
            };
            task_manager.create(task);
            task_manager.register_cancellation(agent_id.clone(), cancellation.clone());

            let provider = self.provider.clone();
            let tools = self.tools.clone();
            let mut child_context = context.clone();
            let agent = agent.clone();
            let mut config = self.config.clone();
            let task_manager_for_task = task_manager.clone();
            let agent_id_for_task = agent_id.clone();
            let prompt = prompt.to_string();

            tokio::spawn(async move {
                config.cancellation = cancellation;
                let result = run_child_agent(
                    provider,
                    tools,
                    config,
                    &agent,
                    &agent_id_for_task,
                    prompt,
                    system_prompt,
                    max_iterations,
                    &mut child_context,
                )
                .await;

                match result {
                    Ok(result) => {
                        task_manager_for_task.complete(&agent_id_for_task, Some(result.final_text))
                    }
                    Err(AgentError::Cancelled) => {
                        task_manager_for_task.cancel(&agent_id_for_task, "cancelled".into())
                    }
                    Err(err) => task_manager_for_task.fail(&agent_id_for_task, err.to_string()),
                }
                task_manager_for_task.unregister_cancellation(&agent_id_for_task);
            });

            return Ok(ToolOutput::json(json!({
                "agent_id": agent_id,
                "task_id": agent_id,
                "agent_type": agent_type_name,
                "description": description,
                "status": "async_launched",
            })));
        }

        let mut context = context;
        let result = run_child_agent(
            self.provider.clone(),
            self.tools.clone(),
            self.config.clone(),
            agent,
            &agent_id,
            prompt.to_string(),
            system_prompt,
            max_iterations,
            &mut context,
        )
        .await?;

        Ok(ToolOutput::json(json!({
            "agent_id": agent_id,
            "agent_type": agent.name,
            "description": description,
            "status": "completed",
            "session_id": result.session_id,
            "final_text": result.final_text,
            "event_count": result.event_count,
        })))
    }
}

async fn run_child_agent(
    provider: std::sync::Arc<dyn crate::provider::ModelProvider + Send + Sync>,
    tools: ToolRegistry,
    mut config: crate::config::AgentConfig,
    agent: &AgentDefinition,
    agent_id: &str,
    prompt: String,
    system_prompt: String,
    max_iterations: Option<usize>,
    context: &mut ToolContext,
) -> Result<SubagentRunResult, AgentError> {
    config.cwd = context.cwd.clone();
    config.env = context.env.clone();
    config.storage = None;
    config.base_system_prompt = Some(system_prompt);
    config.prompt_assembly = None;
    if let Some(max_iterations) = max_iterations {
        config.max_iterations = max_iterations;
    }

    let mut session = AgentSession::new(config)?;
    let child_tools = filter_tools_for_agent(&tools, agent);
    let mut events = Vec::new();
    {
        let erased_provider = crate::provider::ErasedProvider(provider.as_ref());
        let mut stream = Box::pin(session.run_turn_stream(&erased_provider, &child_tools, prompt));
        while let Some(event) = stream.next().await {
            let event = event?;
            if let Some((progress, data)) = progress_summary(&event, agent_id, &agent.name)
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

    Ok(SubagentRunResult {
        session_id: session.session_id().to_string(),
        final_text,
        event_count: events.len(),
    })
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
