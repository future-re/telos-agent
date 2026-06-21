use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::config::CancellationState;
use crate::error::AgentError;
use crate::runtime::{AgentSession, TurnEvent};
use crate::subagent::tool::SubagentTool;
use crate::subagent::{AgentDefinition, create_subagent_worktree};
use crate::tasks::{Task, TaskStatus};
use crate::tool::{ToolContext, ToolOutput, ToolProgress, ToolRegistry};

struct SubagentRunResult {
    session_id: String,
    final_text: String,
    event_count: usize,
    tool_call_count: usize,
}

/// Max characters of subagent output returned to the parent agent.
/// Longer output is truncated to prevent subagent results from
/// flooding the parent's context window.
const MAX_SUBAGENT_RESULT_CHARS: usize = 2_000;

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
        let effective_prompt = match agent.initial_prompt.as_deref() {
            Some(initial_prompt) => format!("{initial_prompt}\n\n{prompt}"),
            None => prompt.to_string(),
        };
        let worktree_path = if matches!(
            arguments.get("isolation").and_then(|value| value.as_str()),
            Some("worktree")
        ) {
            Some(create_subagent_worktree(&context.cwd, &agent_id)?.path)
        } else {
            None
        };

        if arguments.get("run_in_background").and_then(|value| value.as_bool()).unwrap_or(false) {
            let task_manager = self.config.task_manager.clone().ok_or_else(|| {
                AgentError::Validation("run_in_background requires AgentConfig.task_manager".into())
            })?;
            let agent_type_name = agent.name.clone();
            let worktree_path_string =
                worktree_path.as_ref().map(|path| path.display().to_string());
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
                worktree_path: worktree_path_string.clone(),
                error: None,
            };
            task_manager.create(task);
            task_manager.register_cancellation(agent_id.clone(), cancellation.clone());

            let provider = self.provider.clone();
            let tools = self.tools.clone();
            let mut child_context = context.clone();
            if let Some(worktree_path) = worktree_path.clone() {
                child_context.cwd = worktree_path;
            }
            let agent = agent.clone();
            let mut config = self.config.clone();
            let task_manager_for_task = task_manager.clone();
            let agent_id_for_task = agent_id.clone();
            let prompt = effective_prompt;

            tokio::spawn(async move {
                config.cancellation = cancellation;
                let result = run_child_agent(
                    provider,
                    tools,
                    config,
                    &agent,
                    &agent_id_for_task,
                    prompt,
                    build_subagent_system_prompt(&agent, &system_prompt),
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
                "worktree_path": worktree_path_string,
            })));
        }

        let mut context = context;
        let worktree_path_string = worktree_path.as_ref().map(|path| path.display().to_string());
        if let Some(worktree_path) = worktree_path {
            context.cwd = worktree_path;
        }
        let result = run_child_agent(
            self.provider.clone(),
            self.tools.clone(),
            self.config.clone(),
            agent,
            &agent_id,
            effective_prompt,
            build_subagent_system_prompt(agent, &system_prompt),
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
            "final_text_truncated": result.final_text.len() >= MAX_SUBAGENT_RESULT_CHARS,
            "event_count": result.event_count,
            "tool_call_count": result.tool_call_count,
            "worktree_path": worktree_path_string,
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
    // Subagent internal tool calls should NOT inherit the parent's approval
    // handler or permission engine. The parent already approved the subagent
    // call itself, and gating every child tool call (Bash, Read, Write, etc.)
    // with interactive prompts would make the subagent unusable.
    config.approval_handler = None;
    config.permission_engine = None;
    if let Some(max_iterations) = max_iterations {
        config.max_iterations = Some(max_iterations);
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

    let tool_call_count = session.messages().iter().flat_map(|m| m.tool_calls()).count();

    // Truncate subagent output to protect the parent's context window.
    // The parent receives a preview + metadata; full output is available
    // in the subagent's session transcript (if storage is configured).
    let final_text = if final_text.len() > MAX_SUBAGENT_RESULT_CHARS {
        final_text.chars().take(MAX_SUBAGENT_RESULT_CHARS).collect()
    } else {
        final_text
    };

    Ok(SubagentRunResult {
        session_id: session.session_id().to_string(),
        final_text,
        event_count: events.len(),
        tool_call_count,
    })
}

pub(super) fn filter_tools_for_agent(
    tools: &ToolRegistry,
    agent: &AgentDefinition,
) -> ToolRegistry {
    let mut allowed = agent.allowed_tools.clone();
    if !agent.skills.is_empty() {
        add_implicit_allowed_tool(&mut allowed, "Skill");
    }
    for tool in ["MemoryRead", "MemoryGrep", "MemoryWrite"] {
        add_implicit_allowed_tool(&mut allowed, tool);
    }
    tools.filtered(&allowed, &agent.disallowed_tools)
}

fn add_implicit_allowed_tool(allowed: &mut Vec<String>, tool: &str) {
    if allowed.is_empty() || allowed.iter().any(|item| item == "*" || item == tool) {
        return;
    }
    allowed.push(tool.to_string());
}

fn build_subagent_system_prompt(agent: &AgentDefinition, base_system_prompt: &str) -> String {
    let mut sections = vec![base_system_prompt.trim().to_string()];

    if !agent.skills.is_empty() {
        sections.push(format!(
            "# Subagent Skills\nDeclared skills: {}.\nBefore doing substantive work, call the Skill tool once for each declared skill that applies to this task. If a declared skill is unavailable, continue with the best local approach and mention the missing skill in your final result.",
            agent.skills.join(", ")
        ));
    }

    sections.push(
        [
            "# Subagent Learning",
            "Use available memory tools to read relevant memory before relying on assumptions when the delegated task references prior project behavior, user preferences, recurring commands, or known workflows.",
            "When the task reveals a durable preference, reusable command, workflow, project fact, or non-obvious implementation pattern, include a `Reusable learning` section in the final answer with concise memory-worthy notes.",
            "Do not write noisy memory for one-off observations. Prefer exact file paths, commands, failure modes, and verification evidence.",
        ]
        .join("\n"),
    );

    sections.push(
        [
            "# Subagent Output Contract",
            "Return only the delegated result. Include: outcome, key files or commands, verification performed, blockers or uncertainty, and any `Reusable learning` notes.",
            "Keep results compact so the parent agent can merge your findings without losing context.",
        ]
        .join("\n"),
    );

    sections.join("\n\n")
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
