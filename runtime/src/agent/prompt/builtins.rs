use async_trait::async_trait;
use std::sync::Arc;

use crate::agent::prompt::{PromptSection, PromptStability};
use crate::config::TaskPath;
use crate::tools::api::ToolRegistry;

mod context;

pub use context::{
    CwdSection, DateSection, GitStatusSection, McpSection, MemorySection, ProfileSection,
    SkillsSection,
};

// ── Identity ──────────────────────────────────────────────

macro_rules! prompt_template {
    ($name:expr) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../prompt/", $name))
    };
}

static IDENTITY_TEMPLATE: &str = prompt_template!("identity.md");
static TONE_STYLE_TEMPLATE: &str = prompt_template!("tone_style.md");
static TASK_GUIDANCE_TEMPLATE: &str = prompt_template!("task_guidance.md");
static SAFETY_TEMPLATE: &str = prompt_template!("safety.md");
static PATH_FAST_TEMPLATE: &str = prompt_template!("path_fast.md");
static PATH_STANDARD_TEMPLATE: &str = prompt_template!("path_standard.md");
static PATH_HEAVY_TEMPLATE: &str = prompt_template!("path_heavy.md");
static TOOL_USAGE_TEMPLATE: &str = prompt_template!("tool_usage.md");

/// Core identity, security policy, and system-level rules.
///
/// Topics:
///   - identity and role
///   - security testing policy
///   - URL generation rule
///   - output display rules
///   - permission mode / policy / compaction notes
pub struct IdentitySection {
    base: Option<String>,
}

impl IdentitySection {
    pub fn new(base_prompt: Option<String>) -> Self {
        Self { base: base_prompt }
    }
}

#[async_trait]
impl PromptSection for IdentitySection {
    fn name(&self) -> &str {
        "identity"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        let base_block = match &self.base {
            Some(base) => format!("\nAdditional instructions from the user:\n{base}"),
            None => String::new(),
        };
        IDENTITY_TEMPLATE.replace("{{BASE}}", &base_block)
    }
}

// ── Tone and Style ────────────────────────────────────────

/// Output style guidance for a terminal coding assistant.
pub struct ToneStyleSection;

#[async_trait]
impl PromptSection for ToneStyleSection {
    fn name(&self) -> &str {
        "tone_style"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        TONE_STYLE_TEMPLATE.to_string()
    }
}

// ── Task Guidance ─────────────────────────────────────────

/// Recommended workflow for software-engineering tasks.
pub struct TaskGuidanceSection;

#[async_trait]
impl PromptSection for TaskGuidanceSection {
    fn name(&self) -> &str {
        "task_guidance"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        TASK_GUIDANCE_TEMPLATE.to_string()
    }
}

// ── Safety ────────────────────────────────────────────────

/// Safety, reversibility, and honest reporting guidance.
pub struct SafetySection;

#[async_trait]
impl PromptSection for SafetySection {
    fn name(&self) -> &str {
        "safety"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        SAFETY_TEMPLATE.to_string()
    }
}

// ── Task Path ──────────────────────────────────────────────

/// Injects path-appropriate behavioural guidance based on the configured
/// [`TaskPath`]. Fast = work directly, Standard = map context + verify,
/// Heavy = design → plan → phased execution.
pub struct PathSection {
    path: TaskPath,
}

impl PathSection {
    pub fn new(path: TaskPath) -> Self {
        Self { path }
    }
}

#[async_trait]
impl PromptSection for PathSection {
    fn name(&self) -> &str {
        "task_path"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        match self.path {
            TaskPath::Fast => PATH_FAST_TEMPLATE.to_string(),
            TaskPath::Standard => PATH_STANDARD_TEMPLATE.to_string(),
            TaskPath::Heavy => PATH_HEAVY_TEMPLATE.to_string(),
        }
    }
}

// ── Tool Usage ────────────────────────────────────────────

/// General tool-selection and parallelism guidance.
pub struct ToolUsageSection;

/// Tool-selection guidance that reflects the registered default shell.
pub struct ShellAwareToolUsageSection {
    tools: Arc<ToolRegistry>,
}

impl ShellAwareToolUsageSection {
    pub fn new(tools: Arc<ToolRegistry>) -> Self {
        Self { tools }
    }

    fn default_shell(&self) -> &'static str {
        if self.tools.definitions().iter().any(|definition| definition.name == "PowerShell") {
            "PowerShell"
        } else {
            "Bash"
        }
    }
}

fn render_tool_usage(shell_tool: &str) -> String {
    TOOL_USAGE_TEMPLATE.replace("{{SHELL}}", shell_tool)
}

#[async_trait]
impl PromptSection for ToolUsageSection {
    fn name(&self) -> &str {
        "tool_usage"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        render_tool_usage("Bash")
    }
}

#[async_trait]
impl PromptSection for ShellAwareToolUsageSection {
    fn name(&self) -> &str {
        "tool_usage"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        render_tool_usage(self.default_shell())
    }
}

// ── Tools ─────────────────────────────────────────────────

pub struct ToolsSection {
    tools: Arc<ToolRegistry>,
}

impl ToolsSection {
    pub fn new(tools: Arc<ToolRegistry>) -> Self {
        Self { tools }
    }
}

#[async_trait]
impl PromptSection for ToolsSection {
    fn name(&self) -> &str {
        "tools"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        let defs = self.tools.definitions();
        if defs.is_empty() {
            return String::new();
        }
        let mut lines = vec!["## Available Tools".to_string()];
        for def in &defs {
            lines.push(format!("- **{}**: {}", def.name, def.description));
        }
        lines.join("\n")
    }
}

// ── Tool Prompts ──────────────────────────────────────────

/// Per-tool behavioral guidance collected from `Tool::prompt_text()`.
pub struct ToolPromptsSection {
    tools: Arc<ToolRegistry>,
}

impl ToolPromptsSection {
    pub fn new(tools: Arc<ToolRegistry>) -> Self {
        Self { tools }
    }
}

#[async_trait]
impl PromptSection for ToolPromptsSection {
    fn name(&self) -> &str {
        "tool_prompts"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        let mut entries: Vec<(String, String)> = self
            .tools
            .iter()
            .filter_map(|(name, tool)| {
                tool.prompt_text().map(|text| (name.clone(), text.to_string()))
            })
            .collect();
        if entries.is_empty() {
            return String::new();
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut lines = vec!["## Tool-specific guidance".to_string()];
        for (name, text) in entries {
            lines.push(format!("### {name}"));
            for line in text.lines() {
                lines.push(line.to_string());
            }
        }
        lines.join("\n")
    }
}
