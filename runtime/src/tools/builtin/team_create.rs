//! `TeamCreate` tool — create a named collaboration team for multi-agent work.
//!
//! Writes a team config to `~/.telos/teams/{name}/config.json` and initializes
//! a shared task list directory at `~/.telos/tasks/{name}/`.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

use crate::error::AgentError;
use crate::orchestration::team::{self, TeamConfig, TeamMember, lead_agent_id};
use crate::tools::api::{Tool, ToolContext, ToolDefinition, ToolOutput};

/// Shared state tracking the currently active team for this session.
#[derive(Debug, Clone, Default)]
pub struct TeamContext {
    pub team_name: Option<String>,
    pub lead_agent_id: Option<String>,
}

pub type SharedTeamContext = Arc<Mutex<TeamContext>>;

/// `TeamCreate` — initialize a new collaboration team.
pub struct TeamCreateTool {
    team_ctx: SharedTeamContext,
}

impl TeamCreateTool {
    pub fn new(team_ctx: SharedTeamContext) -> Self {
        Self { team_ctx }
    }
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "TeamCreate".into(),
            description: "Create a collaboration team for parallel multi-agent work. \
Use when a task can be split into independent subtasks. Creates shared task list and config. One team at a time."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "team_name": {
                        "type": "string",
                        "description": "A short kebab-case name for the team (e.g. 'refactor-auth', 'api-migration')"
                    },
                    "description": {
                        "type": "string",
                        "description": "What this team is working on — shared context for all members"
                    },
                    "agent_type": {
                        "type": "string",
                        "description": "The subagent_type to use when spawning teammates (e.g. 'general-purpose', 'Explore')"
                    }
                },
                "required": ["team_name"]
            }),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(TEAM_CREATE_PROMPT)
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let mut team_name = arguments
            .get("team_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::Validation("missing `team_name`".into()))?
            .to_string();

        // Check we're not already on a team.
        {
            let ctx = self.team_ctx.lock().unwrap();
            if ctx.team_name.is_some() {
                return Err(AgentError::Validation(
                    "Already managing a team. Call TeamDelete first before creating a new team."
                        .into(),
                ));
            }
        }

        // Ensure team name is unique — append suffix if already exists.
        if team::team_config_path(&team_name).map(|p| p.exists()).unwrap_or(false) {
            team_name = format!("{team_name}-{}", short_id());
        }

        let description =
            arguments.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let lead_id = lead_agent_id(&team_name);

        let config = TeamConfig {
            name: team_name.clone(),
            description: if description.is_empty() { None } else { Some(description) },
            created_at: now,
            lead_agent_id: lead_id.clone(),
            lead_session_id: context.session_id.clone(),
            members: vec![TeamMember {
                agent_id: lead_id.clone(),
                name: "team-lead".into(),
                agent_type: arguments.get("agent_type").and_then(|v| v.as_str()).map(String::from),
                joined_at: now,
                is_active: true,
            }],
        };

        team::save_team_config(&config)?;

        // Initialize shared task directory.
        if let Some(tasks_dir) = team::team_tasks_dir(&team_name) {
            let _ = std::fs::create_dir_all(&tasks_dir);
        }

        // Set team context.
        {
            let mut ctx = self.team_ctx.lock().unwrap();
            ctx.team_name = Some(team_name.clone());
            ctx.lead_agent_id = Some(lead_id.clone());
        }

        Ok(ToolOutput::json(json!({
            "team_name": team_name,
            "lead_agent_id": lead_id,
            "message": format!(
                "Team '{team_name}' created. Lead agent: {lead_id}. \
        Next steps: 1) Create tasks using TaskCreate or TodoWrite. \
        2) Spawn teammates using Subagent tool with this team's task list. \
        3) Teammates will discover the team via ~/.telos/teams/{team_name}/config.json"
            )
        })))
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
}

fn short_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{:x}", now.as_nanos() & 0xFFF)
}

const TEAM_CREATE_PROMPT: &str = r#"TeamCreate initializes a multi-agent collaboration team.

Use when the task has 3+ independent subtasks or when different agent types should work in parallel.

Workflow: TeamCreate → create tasks → spawn teammates via Subagent → collect results → TeamDelete.

Agent types: 'Explore' (codebase research), 'general-purpose' (implementation), 'Plan' (architecture), 'Debug' (debugging). Only ONE team at a time."#;
