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

pub use assembly::PromptAssembly;
pub use builtins::{
    CwdSection, DateSection, GitStatusSection, IdentitySection, McpSection, MemorySection,
    PathSection, ProfileSection, SafetySection, ShellAwareToolUsageSection, SkillsSection,
    TaskGuidanceSection, ToneStyleSection, ToolPromptsSection, ToolUsageSection, ToolsSection,
};
pub use section::{CacheHint, PromptBlock, PromptSection, PromptStability};

use crate::config::TaskPath;
use crate::tool::ToolRegistry;

/// Build a standard coding-agent prompt assembly.
///
/// This is the recommended default for software-engineering sessions. It
/// includes identity, tone/style, task guidance, safety rules, tool-usage
/// guidance, the available tool catalog, the current date, and the working
/// directory.
///
/// Optional sections such as skills, memory, profiles, MCP tools, and git
/// status can be added afterwards by the caller.
pub fn default_coding_assembly(
    tools: Arc<ToolRegistry>,
    cwd: PathBuf,
    skills: Option<Arc<crate::skills::SkillRegistry>>,
    path: TaskPath,
) -> PromptAssembly {
    let mut assembly = PromptAssembly::new();
    assembly.add(IdentitySection::new(None));
    assembly.add(ToneStyleSection);
    assembly.add(TaskGuidanceSection);
    assembly.add(SafetySection);
    assembly.add(PathSection::new(path));
    assembly.add(ShellAwareToolUsageSection::new(Arc::clone(&tools)));
    assembly.add(ToolsSection::new(Arc::clone(&tools)));
    assembly.add(ToolPromptsSection::new(Arc::clone(&tools)));
    assembly.add(DateSection);
    assembly.add(CwdSection::new(cwd));
    if let Some(skills) = skills {
        assembly.add(SkillsSection::new(skills));
    }
    assembly
}
