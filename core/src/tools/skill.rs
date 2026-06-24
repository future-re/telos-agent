use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::error::AgentError;
use crate::skills::SkillRegistry;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

/// Tool that lets the model invoke a user-defined skill by name.
/// The skill's prompt (with {{args}} substituted) is returned to the model.
pub struct SkillTool {
    registry: Arc<SkillRegistry>,
}

impl SkillTool {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Skill".into(),
            description: "Invoke a user-defined skill by name. Returns the skill's prompt for the model to follow.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill": { "type": "string", "description": "The name of the skill to invoke" },
                    "args": { "type": "string", "description": "Optional arguments to pass to the skill" }
                },
                "required": ["skill"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["skill"]
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use the Skill tool to invoke loaded skills by name. Only invoke skills that were explicitly listed in the prompt or recommended via system reminders; do not guess. \
Pass `args` when the skill expects arguments. The skill returns its prompt and body for you to follow.",
        )
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Allow)
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        true
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let skill_name = arguments
            .get("skill")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::Validation("missing string `skill`".into()))?;
        let skill = self
            .registry
            .get(skill_name)
            .ok_or_else(|| AgentError::ToolNotFound(format!("skill `{skill_name}` not found")))?;
        let args = arguments.get("args").and_then(|v| v.as_str()).unwrap_or("");
        let rendered_prompt = skill.prompt.replace("{{args}}", args);
        let full = format!("{}\n\n---\n\n{}", rendered_prompt, skill.body);
        Ok(ToolOutput::json(json!({
            "text": full,
            "skill_name": skill_name,
            "skill_description": skill.description,
        })))
    }
}
