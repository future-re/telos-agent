use async_trait::async_trait;
use std::path::Path;
use telos_agent::prompt::{PromptAssembly, PromptSection, PromptStability};

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

/// Load project context from `root` by discovering instructions files
/// and capturing git status.
pub fn load_project_context(root: &Path) -> ProjectContext {
    let instructions = load_instructions_file(root);
    let git_status = load_git_status(root);

    ProjectContext {
        instructions_file: instructions.as_ref().map(|(name, _)| name.clone()),
        project_instructions: instructions.map(|(_, content)| content),
        git_status,
    }
}

/// Search for a project instructions file (CLAUDE.md, AGENTS.md, etc.)
/// in `root`. Returns `(filename, content)` for the first file found.
fn load_instructions_file(root: &Path) -> Option<(String, String)> {
    for name in &["CLAUDE.md", "AGENTS.md", "CODEBUDDY.md", "GEMINI.md"] {
        let path = root.join(name);
        if path.exists()
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            return Some((name.to_string(), content));
        }
    }
    None
}

/// Capture `git status --short` output from `root` (if it's a git repo).
fn load_git_status(root: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(root)
        .output()
        .ok()?;
    if output.status.success() { String::from_utf8(output.stdout).ok() } else { None }
}

/// A simple static-text section rendered from a pre-loaded string.
#[derive(Debug)]
struct StaticTextSection {
    name: String,
    text: String,
}

#[async_trait]
impl PromptSection for StaticTextSection {
    fn name(&self) -> &str {
        &self.name
    }

    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        self.text.clone()
    }
}

/// Build a [`PromptAssembly`] from a [`ProjectContext`].
///
/// Adds the project instructions file content and the current git status
/// as separate static sections.
pub fn build_prompt_assembly(ctx: &ProjectContext) -> PromptAssembly {
    let mut assembly = PromptAssembly::new();

    if let Some(ref instructions) = ctx.project_instructions {
        let file = ctx.instructions_file.as_deref().unwrap_or("unknown");
        assembly.add(StaticTextSection {
            name: "ProjectInstructions".into(),
            text: format!("## Project Instructions (from {})\n\n{}", file, instructions),
        });
    }

    if let Some(ref status) = ctx.git_status {
        assembly.add(StaticTextSection {
            name: "GitStatus".into(),
            text: format!("## Git Status\n\n```\n{}\n```", status),
        });
    }

    assembly
}

/// Build the status-bar text shown when the TUI launches.
pub fn build_status_text(
    _model: Option<&str>,
    project_root: Option<&Path>,
    ctx: &ProjectContext,
) -> String {
    let project_name = project_root
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "?".to_string());
    format!(
        "telos · {} · {}",
        project_name,
        ctx.instructions_file.as_deref().unwrap_or("no project docs")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_claude_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "be concise").unwrap();
        let ctx = load_project_context(dir.path());
        assert_eq!(ctx.instructions_file.as_deref(), Some("CLAUDE.md"));
        assert_eq!(ctx.project_instructions.as_deref(), Some("be concise"));
    }

    #[test]
    fn falls_back_to_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "use anyhow").unwrap();
        let ctx = load_project_context(dir.path());
        assert_eq!(ctx.instructions_file.as_deref(), Some("AGENTS.md"));
    }

    #[test]
    fn preference_order_claude_over_agents() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "claude rules").unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "agents rules").unwrap();
        let ctx = load_project_context(dir.path());
        assert_eq!(ctx.instructions_file.as_deref(), Some("CLAUDE.md"));
        assert_eq!(ctx.project_instructions.as_deref(), Some("claude rules"));
    }

    #[test]
    fn empty_when_no_instructions_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = load_project_context(dir.path());
        assert!(ctx.instructions_file.is_none());
        assert!(ctx.project_instructions.is_none());
    }

    #[test]
    fn git_status_none_outside_repo() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = load_project_context(dir.path());
        // A temp dir is not a git repo, so git_status should be None
        assert!(ctx.git_status.is_none());
    }

    #[test]
    fn git_status_captured_inside_repo() {
        let dir = tempfile::tempdir().unwrap();

        // Init a minimal git repo
        let output = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .expect("git init must succeed");
        assert!(output.status.success());

        // Configure a minimal user so `git commit` doesn't fail
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output();

        // Create an untracked file
        let mut f = std::fs::File::create(dir.path().join("new.txt")).unwrap();
        writeln!(f, "hello").unwrap();

        let ctx = load_project_context(dir.path());
        let status = ctx.git_status.expect("git_status should be Some in a git repo");
        // Should contain the untracked new.txt file
        assert!(
            status.contains("new.txt"),
            "git status should mention the untracked file, got: {status}"
        );
    }

    #[test]
    fn build_assembly_includes_instructions_and_git() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CODEBUDDY.md"), "buddy instructions").unwrap();

        let ctx = load_project_context(dir.path());
        let assembly = build_prompt_assembly(&ctx);

        // We can't easily inspect the assembly's sections, but we can
        // verify it builds to a string containing our content.
        let built = tokio::runtime::Runtime::new().unwrap().block_on(assembly.build());

        assert!(built.contains("CODEBUDDY.md"));
        assert!(built.contains("buddy instructions"));
    }

    #[test]
    fn build_assembly_empty_when_no_context() {
        let ctx = ProjectContext::empty();
        let assembly = build_prompt_assembly(&ctx);

        let built = tokio::runtime::Runtime::new().unwrap().block_on(assembly.build());
        assert_eq!(built, "");
    }

    #[test]
    fn empty_returns_default() {
        let ctx = ProjectContext::empty();
        assert!(ctx.project_instructions.is_none());
        assert!(ctx.instructions_file.is_none());
        assert!(ctx.git_status.is_none());
    }

    #[test]
    fn status_text_shows_filename() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "x").unwrap();
        let ctx = load_project_context(dir.path());
        let text = build_status_text(Some("deepseek"), Some(dir.path()), &ctx);
        assert!(text.contains("CLAUDE.md"));
    }

    #[test]
    fn status_text_fallback_no_project_docs() {
        let ctx = ProjectContext::empty();
        let text = build_status_text(None, None, &ctx);
        assert!(text.contains("no project docs"));
    }
}
