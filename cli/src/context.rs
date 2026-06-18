use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct ProjectContext {
    pub project_instructions: Option<String>,
    pub instructions_file: Option<String>,
    pub git_status: Option<String>,
}

impl ProjectContext {
    pub fn empty() -> Self {
        Self::default()
    }
}

pub fn load_project_context(_root: &Path) -> ProjectContext {
    ProjectContext::default()
}

/// Build the status-bar text shown when the TUI launches.
pub fn build_status_text(
    model: Option<&str>,
    project_root: Option<&Path>,
    ctx: &ProjectContext,
) -> String {
    let model = model.unwrap_or("default");
    let project_name = project_root
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "?".to_string());
    format!(
        "telos · {} · {} · {}",
        model,
        project_name,
        ctx.instructions_file.as_deref().unwrap_or("no project docs")
    )
}
