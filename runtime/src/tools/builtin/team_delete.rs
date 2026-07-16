//! `TeamDelete` tool — clean up a collaboration team.
//!
//! Removes the team config file and shared task directory. Fails if there are
//! still active (non-lead) team members to prevent data loss.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::orchestration::team::{self, has_active_members, load_team_config};
use crate::tools::api::{Tool, ToolContext, ToolDefinition, ToolOutput};

use super::team_create::SharedTeamContext;

/// `TeamDelete` — tear down the current team.
pub struct TeamDeleteTool {
    team_ctx: SharedTeamContext,
}

impl TeamDeleteTool {
    pub fn new(team_ctx: SharedTeamContext) -> Self {
        Self { team_ctx }
    }
}

#[async_trait]
impl Tool for TeamDeleteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "TeamDelete".into(),
            description: "Clean up the current collaboration team. Removes team config and \
shared task directory. Fails if teammates are still active — ask them to finish \
or stop them first. Use this when the team's work is complete."
                .into(),
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(TEAM_DELETE_PROMPT)
    }

    async fn invoke(
        &self,
        _arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let team_name = {
            let ctx = self.team_ctx.lock().unwrap();
            match &ctx.team_name {
                Some(name) => name.clone(),
                None => {
                    return Ok(ToolOutput::json(json!({
                        "success": true,
                        "message": "No active team to clean up."
                    })));
                }
            }
        };

        // Load and validate before cleanup.
        match load_team_config(&team_name) {
            Ok(config) => {
                if has_active_members(&config) {
                    let active: Vec<_> = config
                        .members
                        .iter()
                        .filter(|m| m.is_active && m.agent_id != config.lead_agent_id)
                        .map(|m| m.name.clone())
                        .collect();
                    return Err(AgentError::Validation(format!(
                        "Cannot clean up team with {} active member(s): {}. \
Stop or wait for them to finish, then retry.",
                        active.len(),
                        active.join(", ")
                    )));
                }
            }
            Err(_) => {
                // Config not found — maybe already cleaned up? Proceed anyway.
            }
        }

        team::cleanup_team(&team_name)?;

        {
            let mut ctx = self.team_ctx.lock().unwrap();
            ctx.team_name = None;
            ctx.lead_agent_id = None;
        }

        Ok(ToolOutput::json(json!({
            "success": true,
            "team_name": team_name,
            "message": format!("Team '{team_name}' cleaned up successfully.")
        })))
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
}

const TEAM_DELETE_PROMPT: &str = r#"TeamDelete cleans up a completed collaboration team.

Use when all tasks are done or the user signals completion. Fails if active teammates remain — stop them first. Removes team config and shared task directory."#;
