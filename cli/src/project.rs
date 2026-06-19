//! Project context detection — finds the project root by walking upward
//! looking for marker files or directories (`.telos.toml` or `.git`).

use std::path::{Path, PathBuf};

/// Walk upward from `start_dir` looking for a project root marker.
///
/// Checks for (in order at each ancestor):
/// 1. A `.telos.toml` file
/// 2. A `.git` directory or file (worktrees use a `.git` file with a `gitdir:` reference)
///
/// Returns the first ancestor directory that contains a marker.
/// If no marker is found, returns the canonicalized `start_dir`.
pub fn find_project_root(start_dir: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    let start_dir = start_dir.as_ref();
    let canonical = start_dir.canonicalize()?;

    let mut current: Option<&Path> = Some(canonical.as_path());

    while let Some(dir) = current {
        // Check for .telos.toml
        if dir.join(".telos.toml").exists() {
            return Ok(dir.to_path_buf());
        }
        // Check for .git (directory) or .git (file, for worktrees)
        let git_path = dir.join(".git");
        if git_path.exists() {
            // It's either a directory or a file (worktree gitdir ref)
            return Ok(dir.to_path_buf());
        }

        // Walk up to parent
        current = dir.parent();
    }

    // Fallback: return the canonical start_dir (this shouldn't normally
    // be reached since we always have a root directory like `/`)
    canonical.canonicalize()
}

/// Read and parse `.telos.toml` from the project root, if it exists.
pub fn read_project_config(project_root: &Path) -> Option<crate::config::FileConfig> {
    let config_path = project_root.join(".telos.toml");
    if !config_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&config_path).ok()?;
    toml::from_str(&content).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn find_root_via_git_dir() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir(root.join(".git")).unwrap();
        let sub = root.join("deep").join("nested");
        fs::create_dir_all(&sub).unwrap();

        let found = find_project_root(&sub).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn find_root_via_git_file() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(root.join(".git"), "gitdir: ../.git/worktrees/foo\n").unwrap();
        let sub = root.join("deep").join("nested");
        fs::create_dir_all(&sub).unwrap();

        let found = find_project_root(&sub).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn find_root_via_telos_toml() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(root.join(".telos.toml"), "[tool]\nprovider = \"mock\"\n").unwrap();
        let sub = root.join("a").join("b").join("c");
        fs::create_dir_all(&sub).unwrap();

        let found = find_project_root(&sub).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn no_marker_returns_start_dir() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let found = find_project_root(root).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn read_config_returns_some_when_file_exists() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(
            root.join(".telos.toml"),
            "[agent]\nprovider = \"deepseek\"\nmodel = \"deepseek-chat\"\n",
        )
        .unwrap();

        let config = read_project_config(root);
        assert!(config.is_some());
        let cfg = config.unwrap();
        let agent = cfg.agent.unwrap();
        assert_eq!(agent.provider.as_deref(), Some("deepseek"));
        assert_eq!(agent.model.as_deref(), Some("deepseek-chat"));
    }

    #[test]
    fn read_config_returns_none_when_missing() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let config = read_project_config(root);
        assert!(config.is_none());
    }
}
