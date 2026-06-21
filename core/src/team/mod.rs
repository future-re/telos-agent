//! Team collaboration — persistent multi-agent project coordination.
//!
//! A Team is a named group of agents that share a task list and communicate
//! via a mailbox. Teams persist on disk at `~/.telos/teams/{name}/config.json`.
//!
//! Key concepts:
//! - **Team** = Project = TaskList (1:1 mapping to shared task directories)
//! - **Lead** — the agent that created the team, coordinates work
//! - **Members** — agents spawned to help with specific tasks
//! - **Mailbox** — async message passing between team members
//!
//! # Lifecycle
//!
//! 1. Lead calls TeamCreate → team file written, shared task list initialized
//! 2. Lead creates tasks via TaskCreate/TodoWrite
//! 3. Lead spawns teammates via SubagentTool with team context
//! 4. Teammates claim tasks, work, and SendUserMessage to coordinate
//! 5. TeamDelete when work is complete (fails if members still active)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// On-disk team configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConfig {
    /// Human-readable team name.
    pub name: String,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// When the team was created (unix timestamp seconds).
    pub created_at: u64,
    /// The lead agent's ID (deterministic: `team-lead@{name}`).
    pub lead_agent_id: String,
    /// The lead agent's session ID.
    pub lead_session_id: String,
    /// Known team members.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<TeamMember>,
}

/// A team member record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    /// Agent ID.
    pub agent_id: String,
    /// Display name.
    pub name: String,
    /// Agent type (e.g. "explore", "general-purpose").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// When this member joined.
    pub joined_at: u64,
    /// Whether this member is still active.
    #[serde(default = "default_true")]
    pub is_active: bool,
}

fn default_true() -> bool {
    true
}

/// Team root directory under the user's home.
pub fn teams_root() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .map(|home| home.join(".telos").join("teams"))
}

/// Path to a team's config file.
pub fn team_config_path(team_name: &str) -> Option<PathBuf> {
    teams_root().map(|root| root.join(team_name).join("config.json"))
}

/// Path to a team's shared task directory.
pub fn team_tasks_dir(team_name: &str) -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .map(|home| home.join(".telos").join("tasks").join(sanitize_name(team_name)))
}

/// Sanitize a team name for use in filesystem paths.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Deterministic lead agent ID.
pub fn lead_agent_id(team_name: &str) -> String {
    format!("team-lead@{}", team_name)
}

/// Load a team config from disk.
pub fn load_team_config(team_name: &str) -> Result<TeamConfig, crate::error::AgentError> {
    let path = team_config_path(team_name).ok_or_else(|| {
        crate::error::AgentError::Config("cannot determine home directory".into())
    })?;

    let content = std::fs::read_to_string(&path).map_err(|e| {
        crate::error::AgentError::Config(format!(
            "team '{}' not found at {}: {}",
            team_name,
            path.display(),
            e
        ))
    })?;

    serde_json::from_str(&content)
        .map_err(|e| crate::error::AgentError::Config(format!("invalid team config: {e}")))
}

/// Save a team config to disk.
pub fn save_team_config(config: &TeamConfig) -> Result<(), crate::error::AgentError> {
    let path = team_config_path(&config.name).ok_or_else(|| {
        crate::error::AgentError::Config("cannot determine home directory".into())
    })?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            crate::error::AgentError::Config(format!("cannot create team directory: {e}"))
        })?;
    }

    let json = serde_json::to_string_pretty(config).map_err(|e| {
        crate::error::AgentError::Config(format!("cannot serialize team config: {e}"))
    })?;

    std::fs::write(&path, json).map_err(|e| {
        crate::error::AgentError::Config(format!(
            "cannot write team config to {}: {}",
            path.display(),
            e
        ))
    })?;

    Ok(())
}

/// Delete a team from disk (config + tasks directory).
pub fn cleanup_team(team_name: &str) -> Result<(), crate::error::AgentError> {
    // Remove config directory.
    if let Some(config_path) = team_config_path(team_name)
        && let Some(parent) = config_path.parent()
        && parent.exists()
    {
        let _ = std::fs::remove_dir_all(parent);
    }

    if let Some(tasks_dir) = team_tasks_dir(team_name)
        && tasks_dir.exists()
    {
        let _ = std::fs::remove_dir_all(&tasks_dir);
    }

    Ok(())
}

/// Check if a team has any active (non-lead) members, which would prevent deletion.
pub fn has_active_members(config: &TeamConfig) -> bool {
    config.members.iter().any(|m| m.is_active && m.agent_id != config.lead_agent_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lead_agent_id_is_deterministic() {
        assert_eq!(lead_agent_id("my-project"), "team-lead@my-project");
        // Same input always gives same output.
        assert_eq!(lead_agent_id("my-project"), lead_agent_id("my-project"));
    }

    #[test]
    fn sanitize_name_replaces_special_chars() {
        assert_eq!(sanitize_name("hello world!"), "hello_world_");
        assert_eq!(sanitize_name("foo/bar"), "foo_bar");
        assert_eq!(sanitize_name("abc-123"), "abc-123");
    }

    #[test]
    fn has_active_members_detects_lead_ignored() {
        let config = TeamConfig {
            name: "test".into(),
            description: None,
            created_at: 0,
            lead_agent_id: "team-lead@test".into(),
            lead_session_id: "s1".into(),
            members: vec![TeamMember {
                agent_id: "team-lead@test".into(),
                name: "lead".into(),
                agent_type: None,
                joined_at: 0,
                is_active: true,
            }],
        };
        assert!(!has_active_members(&config));

        let config_with_active = TeamConfig {
            members: vec![
                TeamMember {
                    agent_id: "team-lead@test".into(),
                    name: "lead".into(),
                    agent_type: None,
                    joined_at: 0,
                    is_active: true,
                },
                TeamMember {
                    agent_id: "worker@test".into(),
                    name: "worker".into(),
                    agent_type: None,
                    joined_at: 0,
                    is_active: true,
                },
            ],
            ..config
        };
        assert!(has_active_members(&config_with_active));
    }
}
