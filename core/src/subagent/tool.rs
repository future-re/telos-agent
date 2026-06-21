//! In-process subagent tool — runs a nested agent session as a tool call.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

mod agent_mode;
mod fork_mode;

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::provider::ModelProvider;
use crate::subagent::SubagentRegistry;
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
Provide a clear 3-5 word description and a self-contained prompt with scope, relevant files, constraints, and expected output. \
Use subagent_type to choose a specialized agent when appropriate: general-purpose, Explore, Plan, or Verification. \
Prefer subagents whose declared skills match the work; the child agent will load those skills with the Skill tool. \
For independent research or verification, launch multiple subagent calls in the same assistant message so they can run in parallel. \
Use run_in_background for long-running work, then inspect results with task_output or stop it with task_stop. \
Use isolation: worktree for write-heavy work that should not touch the parent checkout. \
Ask subagents to return reusable learning when the result contains durable memory-worthy facts, commands, workflows, or preferences. \
Do not duplicate work already being performed in the parent session.",
        )
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        arguments
            .get("prompt")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AgentError::Validation("missing string `prompt`".into()))?;

        if arguments.get("run_in_background").and_then(|value| value.as_bool()).unwrap_or(false)
            && self.config.task_manager.is_none()
        {
            return Err(AgentError::Validation(
                "run_in_background requires AgentConfig.task_manager".into(),
            ));
        }

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

        if mode == "fork"
            && matches!(
                arguments.get("isolation").and_then(|value| value.as_str()),
                Some("worktree")
            )
        {
            return Err(AgentError::Validation(
                "worktree isolation is only supported in agent mode".into(),
            ));
        }

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
        // Subagent is a pure delegation mechanism — its child tools run inside
        // a child AgentSession that inherits the parent's approval_handler via
        // config.clone(). Every child tool call already goes through the normal
        // approval flow (auto-allow, safety checks, human prompts). Requiring
        // separate approval for the subagent call itself is redundant and causes
        // the parent tool phase to block while waiting for a decision that
        // conveys no additional security benefit.
        Ok(PermissionDecision::Allow)
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
            _ => self.run_agent(arguments, context).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockProvider;
    use crate::subagent::{AgentDefinition, AgentSource};
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
    async fn validate_rejects_background_without_task_manager_and_fork_worktree() {
        let tool = SubagentTool::new(
            Arc::new(MockProvider::new(vec![])),
            ToolRegistry::new(),
            AgentConfig::default(),
        );

        let background = tool
            .validate(
                &json!({
                    "prompt": "inspect",
                    "run_in_background": true
                }),
                &ToolContext::dummy(),
            )
            .await
            .unwrap_err();
        assert!(background.to_string().contains("requires AgentConfig.task_manager"));

        let worktree = tool
            .validate(
                &json!({
                    "prompt": "inspect",
                    "mode": "fork",
                    "isolation": "worktree",
                    "forks": [{"lens": "a", "system_prompt": "a", "task": "a"}]
                }),
                &ToolContext::dummy(),
            )
            .await
            .unwrap_err();
        assert!(worktree.to_string().contains("only supported in agent mode"));
    }

    #[test]
    fn subagent_prompt_text_describes_distribution_controls() {
        let tool = SubagentTool::new(
            Arc::new(MockProvider::new(vec![])),
            ToolRegistry::new(),
            AgentConfig::default(),
        );
        let prompt = tool.prompt_text().unwrap();
        assert!(prompt.contains("self-contained prompt"));
        assert!(prompt.contains("multiple subagent calls"));
        assert!(prompt.contains("run_in_background"));
        assert!(prompt.contains("task_output"));
        assert!(prompt.contains("task_stop"));
        assert!(prompt.contains("isolation: worktree"));
    }

    #[test]
    fn subagent_prompt_text_describes_skill_and_learning_controls() {
        let tool = SubagentTool::new(
            Arc::new(MockProvider::new(vec![])),
            ToolRegistry::new(),
            AgentConfig::default(),
        );
        let prompt = tool.prompt_text().unwrap();
        assert!(prompt.contains("skills"));
        assert!(prompt.contains("Skill"));
        assert!(prompt.contains("learning"));
        assert!(prompt.contains("memory"));
    }

    #[tokio::test]
    async fn subagent_type_selects_agent_prompt_and_result_metadata() {
        let provider = Arc::new(MockProvider::new(vec![crate::provider::CompletionResponse {
            message: crate::Message::assistant("explore result"),
            stop_reason: crate::provider::StopReason::EndTurn,
            usage: None,
            model: None,
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
    async fn subagent_initial_prompt_is_prepended_to_delegated_prompt() {
        let provider = Arc::new(MockProvider::new(vec![crate::provider::CompletionResponse {
            message: crate::Message::assistant("done"),
            stop_reason: crate::provider::StopReason::EndTurn,
            usage: None,
            model: None,
        }]));
        let mut registry = SubagentRegistry::new();
        let mut agent = AgentDefinition::new(
            "preflight",
            "Preflight agent",
            "You prepare before work.",
            AgentSource::BuiltIn,
        );
        agent.initial_prompt = Some("Read CONTRIBUTING.md first.".into());
        registry.register(agent);
        let tool = SubagentTool::with_registry(
            provider.clone(),
            ToolRegistry::new(),
            AgentConfig::default(),
            registry,
        );

        tool.invoke(
            json!({
                "description": "Run preflight",
                "prompt": "Inspect parser",
                "subagent_type": "preflight"
            }),
            ToolContext::dummy(),
        )
        .await
        .unwrap();

        let requests = provider.requests.lock().await;
        let user_text = requests[0]
            .messages
            .iter()
            .find(|message| message.role == crate::Role::User)
            .expect("request should include delegated user prompt")
            .text_content();
        assert!(user_text.starts_with("Read CONTRIBUTING.md first."));
        assert!(user_text.contains("Inspect parser"));
    }

    #[tokio::test]
    async fn subagent_declared_skills_are_injected_into_system_prompt() {
        let provider = Arc::new(MockProvider::new(vec![crate::provider::CompletionResponse {
            message: crate::Message::assistant("done"),
            stop_reason: crate::provider::StopReason::EndTurn,
            usage: None,
            model: None,
        }]));
        let mut registry = SubagentRegistry::new();
        let mut agent = AgentDefinition::new(
            "debugger",
            "Debugging agent",
            "You debug the delegated issue.",
            AgentSource::BuiltIn,
        );
        agent.skills = vec!["debug".into(), "verify".into()];
        registry.register(agent);
        let tool = SubagentTool::with_registry(
            provider.clone(),
            ToolRegistry::new(),
            AgentConfig::default(),
            registry,
        );

        tool.invoke(
            json!({
                "description": "Debug issue",
                "prompt": "Find the bug",
                "subagent_type": "debugger"
            }),
            ToolContext::dummy(),
        )
        .await
        .unwrap();

        let requests = provider.requests.lock().await;
        let system_prompt = requests[0].system_prompt.as_deref().unwrap_or_default();
        assert!(system_prompt.contains("# Subagent Skills"), "{system_prompt}");
        assert!(system_prompt.contains("Skill tool"), "{system_prompt}");
        assert!(system_prompt.contains("debug"), "{system_prompt}");
        assert!(system_prompt.contains("verify"), "{system_prompt}");
    }

    #[tokio::test]
    async fn subagent_system_prompt_contains_learning_contract() {
        let provider = Arc::new(MockProvider::new(vec![crate::provider::CompletionResponse {
            message: crate::Message::assistant("done"),
            stop_reason: crate::provider::StopReason::EndTurn,
            usage: None,
            model: None,
        }]));
        let tool = SubagentTool::new(provider.clone(), ToolRegistry::new(), AgentConfig::default());

        tool.invoke(
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
        assert!(system_prompt.contains("# Subagent Learning"), "{system_prompt}");
        assert!(system_prompt.contains("memory"), "{system_prompt}");
        assert!(system_prompt.contains("Reusable learning"), "{system_prompt}");
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
                model: None,
            },
            crate::provider::CompletionResponse {
                message: crate::Message::assistant("done"),
                stop_reason: crate::provider::StopReason::EndTurn,
                usage: None,
                model: None,
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

        let filtered = agent_mode::filter_tools_for_agent(&tools, &agent);
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

        let filtered = agent_mode::filter_tools_for_agent(&tools, &agent);

        assert!(filtered.get("Read").is_ok());
        assert!(filtered.get("Write").is_err());
    }

    #[test]
    fn declared_skills_keep_skill_tool_available_with_allowlist() {
        let mut tools = ToolRegistry::new();
        tools.register(NamedTool("Read"));
        tools.register(NamedTool("Skill"));
        tools.register(NamedTool("MemoryRead"));
        tools.register(NamedTool("MemoryWrite"));
        tools.register(NamedTool("MemoryGrep"));

        let mut agent = AgentDefinition::new("skilled", "skilled", "prompt", AgentSource::BuiltIn);
        agent.allowed_tools = vec!["Read".into()];
        agent.skills = vec!["debug".into()];

        let filtered = agent_mode::filter_tools_for_agent(&tools, &agent);

        assert!(filtered.get("Read").is_ok());
        assert!(filtered.get("Skill").is_ok());
        assert!(filtered.get("MemoryRead").is_ok());
        assert!(filtered.get("MemoryWrite").is_ok());
        assert!(filtered.get("MemoryGrep").is_ok());
    }
}
