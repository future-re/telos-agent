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

pub fn load_project_context(root: &Path) -> ProjectContext {
    let instructions = load_instructions_file(root);
    let git_status = load_git_status(root);

    ProjectContext {
        instructions_file: instructions.as_ref().map(|(name, _)| name.clone()),
        project_instructions: instructions.map(|(_, content)| content),
        git_status,
    }
}

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

fn load_git_status(root: &Path) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command.args(["status", "--short"]).current_dir(root);
    hide_console_window(&mut command);
    let output = command.output().ok()?;
    if output.status.success() { String::from_utf8(output.stdout).ok() } else { None }
}

fn hide_console_window(_command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

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

pub fn build_prompt_assembly(ctx: &ProjectContext) -> PromptAssembly {
    let mut assembly = PromptAssembly::new();
    append_prompt_context(&mut assembly, ctx);
    assembly
}

pub fn append_prompt_context(assembly: &mut PromptAssembly, ctx: &ProjectContext) {
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
}

pub fn build_status_text(
    model: Option<&str>,
    project_root: Option<&Path>,
    ctx: &ProjectContext,
) -> String {
    let mut parts: Vec<String> = vec!["telos".to_string()];
    if let Some(name) =
        project_root.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string())
        && name != "?"
    {
        parts.push(name);
    }
    let tag = model.map(str::to_string).or_else(|| ctx.instructions_file.clone());
    if let Some(tag) = tag
        && tag != "?"
    {
        parts.push(tag);
    }
    parts.join(" · ")
}
