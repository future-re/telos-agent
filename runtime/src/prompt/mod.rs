//! Prompt system — modular, cache-aware construction of the system prompt.
//!
//! The prompt is assembled from independent sections rather than one hardcoded
//! string. Static sections are rendered once and cached; dynamic sections are
//! re-rendered every turn. This mirrors the modular prompt architecture:
//! "prompt is assembled, not hardcoded".
pub mod assembly;
pub mod builtins;
pub mod section;

use std::path::PathBuf;
use std::sync::Arc;

pub use assembly::{PromptAssembly, PromptSectionStat, PromptStats};
pub use builtins::{
    CwdSection, DateSection, GitStatusSection, IdentitySection, McpSection, MemorySection,
    PathSection, ProfileSection, SafetySection, ShellAwareToolUsageSection, SkillsSection,
    TaskGuidanceSection, ToneStyleSection, ToolPromptsSection, ToolUsageSection, ToolsSection,
};
pub use section::{CacheHint, PromptBlock, PromptSection, PromptStability};

use crate::config::TaskPath;
use crate::tool::ToolRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptProfile {
    #[default]
    Minimal,
    Full,
}

/// Build a standard coding-agent prompt assembly.
///
/// This is the recommended default for software-engineering sessions. It
/// includes the minimum shared guidance needed for software-engineering
/// sessions: identity, safety rules, task path, the current date, and the
/// working directory.
///
/// Optional sections such as skills, memory, profiles, MCP tools, and git
/// status can be added afterwards by the caller.
pub fn default_coding_assembly(
    tools: Arc<ToolRegistry>,
    cwd: PathBuf,
    skills: Option<Arc<crate::skills::SkillRegistry>>,
    path: TaskPath,
) -> PromptAssembly {
    default_coding_assembly_for_profile(tools, cwd, skills, path, PromptProfile::Minimal)
}

pub fn default_coding_assembly_for_profile(
    tools: Arc<ToolRegistry>,
    cwd: PathBuf,
    skills: Option<Arc<crate::skills::SkillRegistry>>,
    path: TaskPath,
    profile: PromptProfile,
) -> PromptAssembly {
    let mut assembly = PromptAssembly::new();
    assembly.add(IdentitySection::new(None));
    assembly.add(SafetySection);
    assembly.add(PathSection::new(path));
    if profile == PromptProfile::Full {
        assembly.add(ToneStyleSection);
        assembly.add(TaskGuidanceSection);
        assembly.add(ShellAwareToolUsageSection::new(Arc::clone(&tools)));
        assembly.add(ToolsSection::new(Arc::clone(&tools)));
        assembly.add(ToolPromptsSection::new(Arc::clone(&tools)));
        if let Some(skills) = skills {
            assembly.add(SkillsSection::new(skills));
        }
    }
    assembly.add(DateSection);
    assembly.add(CwdSection::new(cwd));
    assembly
}
