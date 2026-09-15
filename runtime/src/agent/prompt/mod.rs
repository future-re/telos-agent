//! Prompt system for assembling the small set of system prompt blocks the
//! runtime needs.
pub mod assembly;
pub mod builtins;
pub mod section;

use std::path::PathBuf;
use std::sync::Arc;

pub use assembly::PromptAssembly;
pub use builtins::{
    CwdSection, DateSection, GitStatusSection, IdentitySection, McpSection, MemorySection,
    ProfileSection, SafetySection, ShellAwareToolUsageSection, SkillsSection, ToolPromptsSection,
};
pub use section::{CacheHint, PromptBlock, PromptSection, PromptStability};

use crate::tools::api::ToolRegistry;

/// Build the default office-work agent prompt assembly.
///
/// This is the recommended default for work sessions. It includes the
/// shared guidance needed for document, research, file, browser, and
/// data-analysis tasks.
///
/// Optional sections such as skills, memory, profiles, MCP tools, and git
/// status can be added afterwards by the caller.
pub fn default_work_assembly(
    tools: Arc<ToolRegistry>,
    cwd: PathBuf,
) -> PromptAssembly {
    let mut assembly = PromptAssembly::new();
    assembly.add(IdentitySection::new(None));
    assembly.add(SafetySection);
    assembly.add(ShellAwareToolUsageSection::new(Arc::clone(&tools)));
    assembly.add(ToolPromptsSection::new(tools));
    assembly.add(DateSection);
    assembly.add(CwdSection::new(cwd));
    assembly
}
