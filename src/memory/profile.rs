use std::io;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::{fs, path::Path};

/// Manages three context profiles: user, project, and active state.
/// Profiles are always injected into the system prompt.
pub struct ProfileManager {
    user_profile_path: PathBuf,
    project_profile_path: PathBuf,
    active_state_path: PathBuf,
}

impl ProfileManager {
    /// Create a ProfileManager.
    /// `user_dir` — user config directory (~/.tiny-agent/profile/)
    /// `project_dir` — project config directory (.tiny-agent/profile/)
    pub fn new(user_dir: PathBuf, project_dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&user_dir)?;
        fs::create_dir_all(&project_dir)?;
        Ok(Self {
            user_profile_path: user_dir.join("user.md"),
            project_profile_path: project_dir.join("project.md"),
            active_state_path: project_dir.join("active.md"),
        })
    }

    /// Read the user profile.
    pub fn user_profile(&self) -> String {
        self.try_user_profile().unwrap_or_else(|err| {
            tracing::warn!(
                path = %self.user_profile_path.display(),
                error = %err,
                "failed to read user profile"
            );
            String::new()
        })
    }

    /// Read the user profile, returning filesystem errors except missing files.
    pub fn try_user_profile(&self) -> io::Result<String> {
        read_optional_profile(&self.user_profile_path)
    }

    /// Read the project profile.
    pub fn project_profile(&self) -> String {
        self.try_project_profile().unwrap_or_else(|err| {
            tracing::warn!(
                path = %self.project_profile_path.display(),
                error = %err,
                "failed to read project profile"
            );
            String::new()
        })
    }

    /// Read the project profile, returning filesystem errors except missing files.
    pub fn try_project_profile(&self) -> io::Result<String> {
        read_optional_profile(&self.project_profile_path)
    }

    /// Read the active state.
    pub fn active_state(&self) -> String {
        self.try_active_state().unwrap_or_else(|err| {
            tracing::warn!(
                path = %self.active_state_path.display(),
                error = %err,
                "failed to read active state"
            );
            String::new()
        })
    }

    /// Read the active state, returning filesystem errors except missing files.
    pub fn try_active_state(&self) -> io::Result<String> {
        read_optional_profile(&self.active_state_path)
    }

    /// Write/update the user profile.
    pub fn set_user_profile(&self, content: &str) -> std::io::Result<()> {
        fs::write(&self.user_profile_path, content)
    }

    /// Write/update the project profile.
    pub fn set_project_profile(&self, content: &str) -> std::io::Result<()> {
        fs::write(&self.project_profile_path, content)
    }

    /// Update the active state — called at session end with a summary.
    pub fn update_active_state(&self, summary: &str) -> std::io::Result<()> {
        let mut content = String::from("## Active Work\n\n");
        content.push_str(summary);
        content.push('\n');
        fs::write(&self.active_state_path, &content)
    }

    /// Canonicalize the user profile file during a consolidation pass.
    ///
    /// This lightweight implementation does not infer facts. It preserves any
    /// existing content, trims surrounding whitespace, and creates an empty
    /// profile skeleton when the file does not exist yet.
    pub fn consolidate_user_profile(&self) -> io::Result<()> {
        consolidate_profile_file(&self.user_profile_path, "user")
    }

    /// Canonicalize the project profile file during a consolidation pass.
    ///
    /// This lightweight implementation does not infer project conventions. It
    /// preserves any existing content, trims surrounding whitespace, and creates
    /// an empty profile skeleton when the file does not exist yet.
    pub fn consolidate_project_profile(&self) -> io::Result<()> {
        consolidate_profile_file(&self.project_profile_path, "project")
    }

    /// Render all three profiles as a combined prompt section.
    pub fn render_all(&self) -> String {
        self.try_render_all().unwrap_or_else(|err| {
            tracing::warn!(error = %err, "failed to render profiles");
            String::new()
        })
    }

    /// Render all three profiles as a combined prompt section.
    pub fn try_render_all(&self) -> io::Result<String> {
        let mut parts = Vec::new();

        let user = self.try_user_profile()?;
        if !user.is_empty() {
            parts.push(format!("## User Profile\n\n{user}"));
        }

        let project = self.try_project_profile()?;
        if !project.is_empty() {
            parts.push(format!("## Project Profile\n\n{project}"));
        }

        let active = self.try_active_state()?;
        if !active.is_empty() {
            parts.push(format!("## Active State\n\n{active}"));
        }

        Ok(parts.join("\n\n"))
    }
}

fn read_optional_profile(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err),
    }
}

fn consolidate_profile_file(path: &Path, profile: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let current = read_optional_profile(path)?;
    let content = if current.trim().is_empty() {
        format!("---\ntype: profile\nprofile: {profile}\n---\n")
    } else {
        format!("{}\n", current.trim())
    };
    fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_user_profile() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap();
        mgr.set_user_profile("Test user profile").unwrap();
        assert_eq!(mgr.user_profile(), "Test user profile");
        assert_eq!(mgr.try_user_profile().unwrap(), "Test user profile");
    }

    #[test]
    fn update_active_state() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap();
        mgr.update_active_state("Working on Phase 1").unwrap();
        assert!(mgr.active_state().contains("Working on Phase 1"));
        assert!(mgr.active_state().contains("Active Work"));
    }

    #[test]
    fn render_all_combines_available_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap();
        mgr.set_user_profile("User pref: short names").unwrap();
        mgr.set_project_profile("Project: Rust 2024").unwrap();
        mgr.update_active_state("Task: building profiles").unwrap();

        let rendered = mgr.render_all();
        assert!(rendered.contains("User Profile"));
        assert!(rendered.contains("User pref: short names"));
        assert!(rendered.contains("Project Profile"));
        assert!(rendered.contains("Active State"));
    }

    #[test]
    fn empty_profiles_render_empty_strings() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap();
        assert_eq!(mgr.user_profile(), "");
        assert_eq!(mgr.project_profile(), "");
        assert_eq!(mgr.render_all(), "");
    }

    #[test]
    fn consolidation_creates_profile_skeletons() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap();

        mgr.consolidate_user_profile().unwrap();
        mgr.consolidate_project_profile().unwrap();

        assert!(mgr.try_user_profile().unwrap().contains("profile: user"));
        assert!(mgr.try_project_profile().unwrap().contains("profile: project"));
    }

    #[test]
    fn consolidation_preserves_existing_content_and_trims() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap();
        mgr.set_user_profile("\n\nExisting profile\n\n").unwrap();

        mgr.consolidate_user_profile().unwrap();

        assert_eq!(mgr.try_user_profile().unwrap(), "Existing profile\n");
    }
}
