use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::AgentError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
}

pub fn create_subagent_worktree(
    parent_cwd: &Path,
    agent_id: &str,
) -> Result<WorktreeInfo, AgentError> {
    let worktree_path = parent_cwd.join(".worktrees").join("subagents").join(agent_id);
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| AgentError::ToolExecution {
            tool: "subagent".into(),
            message: format!("failed to create worktree parent {}: {err}", parent.display()),
        })?;
    }

    let output = Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(&worktree_path)
        .arg("HEAD")
        .current_dir(parent_cwd)
        .output()
        .map_err(|err| AgentError::ToolExecution {
            tool: "subagent".into(),
            message: format!("failed to run git worktree add: {err}"),
        })?;

    if !output.status.success() {
        return Err(AgentError::ToolExecution {
            tool: "subagent".into(),
            message: format!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }

    Ok(WorktreeInfo { path: worktree_path })
}
