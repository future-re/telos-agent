//! Git worktree isolation for subagents — creation, fast-resume, cleanup.
//!
//! Provides safe worktree creation with slug validation, canonical git-root
//! resolution, fast-resume, dirty-state detection, and cleanup. Designed to
//! prevent worktree nesting and protect the main repo from accidental mutation
//! by isolated subagent runs.

#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::AgentError;

/// Info about a created or resumed worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree working tree.
    pub path: PathBuf,
    /// Branch name (None when using --detach).
    pub branch: Option<String>,
    /// Whether this worktree already existed (fast-resume path).
    pub was_existing: bool,
}

/// Create or resume a git worktree for a subagent.
///
/// Creates the worktree at `<git_root>/.worktrees/subagents/<agent_id>`.
/// If a worktree already exists at that path, it is resumed (fast path).
/// Uses a detached HEAD for simplicity.
pub fn create_subagent_worktree(
    parent_cwd: &Path,
    agent_id: &str,
) -> Result<WorktreeInfo, AgentError> {
    // Validate the agent_id as a safe slug to prevent path traversal.
    validate_slug(agent_id)?;

    // Find the canonical (top-level) git root, NOT a sub-worktree's root.
    let git_root = find_canonical_git_root(parent_cwd)?;

    let worktree_dir = git_root.join(".worktrees").join("subagents").join(agent_id);

    // Fast-resume: check if the worktree already exists.
    if worktree_dir.exists() {
        // Verify it's actually a git worktree (has a .git file pointing back).
        let git_file = worktree_dir.join(".git");
        if git_file.exists()
            && std::fs::read_to_string(&git_file).unwrap_or_default().contains("gitdir:")
        {
            return Ok(WorktreeInfo { path: worktree_dir, branch: None, was_existing: true });
        }
        // Corrupt — remove and recreate.
        let _ = std::fs::remove_dir_all(&worktree_dir);
    }

    // Create parent directories.
    std::fs::create_dir_all(
        worktree_dir
            .parent()
            .ok_or_else(|| AgentError::Config("worktree parent directory not found".into()))?,
    )
    .map_err(|e| AgentError::Config(format!("failed to create worktree parent dir: {e}")))?;

    // Create the worktree with detached HEAD.
    let mut command = hidden_command("git");
    let output = command
        .args([
            "worktree",
            "add",
            "--detach",
            worktree_dir
                .to_str()
                .ok_or_else(|| AgentError::Config("worktree path is not valid UTF-8".into()))?,
            "HEAD",
        ])
        .current_dir(&git_root)
        .output()
        .map_err(|e| AgentError::Config(format!("failed to run git worktree add: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AgentError::Config(format!("git worktree add failed: {}", stderr.trim())));
    }

    Ok(WorktreeInfo { path: worktree_dir, branch: None, was_existing: false })
}

/// Clean up a subagent worktree.
///
/// Removes the worktree from disk (via `git worktree remove --force`) and
/// prunes stale worktree metadata. If `safety_check` is true, checks for
/// uncommitted changes before removing and returns an error if the worktree
/// is dirty.
pub fn remove_subagent_worktree(
    worktree_path: &Path,
    safety_check: bool,
) -> Result<(), AgentError> {
    if !worktree_path.exists() {
        return Ok(()); // Nothing to clean up.
    }

    if safety_check && has_worktree_changes(worktree_path)? {
        return Err(AgentError::Config(format!(
            "worktree at {} has uncommitted changes — refusing to remove",
            worktree_path.display()
        )));
    }

    // Run git worktree remove
    let mut command = hidden_command("git");
    let output = command
        .args(["worktree", "remove", "--force", worktree_path.to_str().unwrap_or("")])
        .output()
        .map_err(|e| AgentError::Config(format!("failed to run git worktree remove: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Fallback: if git worktree remove fails, just delete the directory.
        if stderr.contains("is not a working tree") || stderr.contains("not found") {
            let _ = std::fs::remove_dir_all(worktree_path);
        } else {
            return Err(AgentError::Config(format!(
                "git worktree remove failed: {}",
                stderr.trim()
            )));
        }
    }

    // Prune stale metadata.
    let _ = hidden_command("git").args(["worktree", "prune"]).output();

    Ok(())
}

/// Check if a worktree has uncommitted changes (modified or untracked files).
pub fn has_worktree_changes(worktree_path: &Path) -> Result<bool, AgentError> {
    let mut command = hidden_command("git");
    let output = command
        .args(["status", "--porcelain", "-uno"])
        .current_dir(worktree_path)
        .output()
        .map_err(|e| AgentError::Config(format!("failed to check git status: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(!stdout.trim().is_empty())
}

fn hidden_command(program: &str) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Clean up stale ephemeral subagent worktrees older than the cutoff.
///
/// Scans `<git_root>/.worktrees/subagents/` for subdirectories matching
/// subagent naming patterns and removes ones that have no uncommitted changes.
pub fn cleanup_stale_subagent_worktrees(
    git_root: &Path,
    _cutoff_seconds: u64,
) -> Result<Vec<PathBuf>, AgentError> {
    let subagents_dir = git_root.join(".worktrees").join("subagents");
    if !subagents_dir.exists() {
        return Ok(Vec::new());
    }

    let mut cleaned = Vec::new();

    let entries = std::fs::read_dir(&subagents_dir)
        .map_err(|e| AgentError::Config(format!("failed to read worktree dir: {e}")))?;

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Skip if dirty.
        match has_worktree_changes(&path) {
            Ok(true) => continue, // Has changes — don't clean up.
            Ok(false) => {}
            Err(_) => continue,
        }

        // Safe to remove.
        if let Ok(()) = remove_subagent_worktree(&path, false) {
            cleaned.push(path);
        }
    }

    Ok(cleaned)
}

/// Find the canonical (top-level) git root, traversing through nested worktrees.
///
/// Unlike `find_git_root` which stops at the first `.git`, this function
/// follows `.git` files (which are used by git worktrees to point back to
/// the main repo) to find the *real* top-level repo. This prevents creating
/// nested worktrees inside existing worktrees.
fn find_canonical_git_root(cwd: &Path) -> Result<PathBuf, AgentError> {
    let mut current = cwd.to_path_buf();

    // First, find any git root (could be a worktree).
    let first_root = loop {
        let git_path = current.join(".git");
        if git_path.exists() {
            break current.clone();
        }
        if !current.pop() {
            return Err(AgentError::Config(format!(
                "not inside a git repository: {}",
                cwd.display()
            )));
        }
    };

    // Read .git to see if it's a worktree (pointer file) or regular repo dir.
    let git_entry = first_root.join(".git");
    if git_entry.is_file()
        && let Ok(content) = std::fs::read_to_string(&git_entry)
    {
        // Format: "gitdir: /path/to/main/.git/worktrees/name"
        if let Some(gitdir_line) = content.strip_prefix("gitdir: ") {
            let gitdir_path = PathBuf::from(gitdir_line.trim());
            // The real .git dir is at `<gitdir>/../../` (parent of the worktrees dir)
            if let Some(worktrees_dir) = gitdir_path.parent()
                && let Some(git_dir) = worktrees_dir.parent()
                && let Some(main_repo) = git_dir.parent()
            {
                return Ok(main_repo.to_path_buf());
            }
        }
    }

    // Not a worktree — this is the canonical root.
    Ok(first_root)
}

/// Validate a slug for use in worktree paths and branch names.
///
/// Rules:
/// - Max 64 characters total
/// - Each `/`-separated segment must contain only `[a-zA-Z0-9._-]`
/// - Segments must not be `.` or `..`
/// - Segments must not be empty
pub fn validate_slug(slug: &str) -> Result<(), AgentError> {
    if slug.is_empty() {
        return Err(AgentError::Config("slug must not be empty".into()));
    }
    if slug.len() > 64 {
        return Err(AgentError::Config(format!("slug too long: max 64 chars, got {}", slug.len())));
    }

    for segment in slug.split('/') {
        if segment.is_empty() {
            return Err(AgentError::Config("slug contains an empty segment".into()));
        }
        if segment == "." || segment == ".." {
            return Err(AgentError::Config(format!("slug contains invalid segment: '{segment}'")));
        }
        if !segment.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-') {
            return Err(AgentError::Config(format!(
                "slug segment '{segment}' contains invalid characters (allowed: a-z, A-Z, 0-9, ., _, -)"
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_slugs() {
        assert!(validate_slug("explore-agent").is_ok());
        assert!(validate_slug("agent-001").is_ok());
        assert!(validate_slug("team/lead").is_ok());
        assert!(validate_slug("a").is_ok());
        assert!(validate_slug("abc.def_ghi-jkl").is_ok());
    }

    #[test]
    fn invalid_slugs() {
        assert!(validate_slug("").is_err());
        assert!(validate_slug(".").is_err());
        assert!(validate_slug("..").is_err());
        assert!(validate_slug("a/../b").is_err());
        assert!(validate_slug("has space").is_err());
        assert!(validate_slug("has/slash/at/end/").is_err());
        assert!(validate_slug("/starts-with-slash").is_err());
    }

    #[test]
    fn slug_max_length() {
        assert!(validate_slug(&"a".repeat(64)).is_ok());
        assert!(validate_slug(&"a".repeat(65)).is_err());
    }
}
