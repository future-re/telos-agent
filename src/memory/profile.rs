use std::fs;
use std::path::PathBuf;

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
    pub fn new(user_dir: PathBuf, project_dir: PathBuf) -> Self {
        fs::create_dir_all(&user_dir).ok();
        fs::create_dir_all(&project_dir).ok();
        Self {
            user_profile_path: user_dir.join("user.md"),
            project_profile_path: project_dir.join("project.md"),
            active_state_path: project_dir.join("active.md"),
        }
    }

    /// Read the user profile.
    pub fn user_profile(&self) -> String {
        fs::read_to_string(&self.user_profile_path).unwrap_or_default()
    }

    /// Read the project profile.
    pub fn project_profile(&self) -> String {
        fs::read_to_string(&self.project_profile_path).unwrap_or_default()
    }

    /// Read the active state.
    pub fn active_state(&self) -> String {
        fs::read_to_string(&self.active_state_path).unwrap_or_default()
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

    /// Render all three profiles as a combined prompt section.
    pub fn render_all(&self) -> String {
        let mut parts = Vec::new();

        let user = self.user_profile();
        if !user.is_empty() {
            parts.push(format!("## User Profile\n\n{user}"));
        }

        let project = self.project_profile();
        if !project.is_empty() {
            parts.push(format!("## Project Profile\n\n{project}"));
        }

        let active = self.active_state();
        if !active.is_empty() {
            parts.push(format!("## Active State\n\n{active}"));
        }

        parts.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_user_profile() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        mgr.set_user_profile("Test user profile").unwrap();
        assert_eq!(mgr.user_profile(), "Test user profile");
    }

    #[test]
    fn update_active_state() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        mgr.update_active_state("Working on Phase 1").unwrap();
        assert!(mgr.active_state().contains("Working on Phase 1"));
        assert!(mgr.active_state().contains("Active Work"));
    }

    #[test]
    fn render_all_combines_available_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf());
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
        let mgr = ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        assert_eq!(mgr.user_profile(), "");
        assert_eq!(mgr.project_profile(), "");
        assert_eq!(mgr.render_all(), "");
    }
}
