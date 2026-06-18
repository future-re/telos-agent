//! Prompt system — modular, cache-aware construction of the system prompt.
//!
//! The prompt is assembled from independent sections rather than one hardcoded
//! string. Static sections are rendered once and cached; dynamic sections are
//! re-rendered every turn. This mirrors the design taught in
//! learn-claude-code: "prompt is assembled, not hardcoded".
pub mod assembly;
pub mod builtins;
pub mod section;

use std::path::PathBuf;
use std::sync::Arc;

pub use assembly::PromptAssembly;
pub use builtins::{
    CwdSection, DateSection, GitStatusSection, IdentitySection, McpSection, MemorySection,
    ProfileSection, SafetySection, SkillsSection, TaskGuidanceSection, ToneStyleSection,
    ToolPromptsSection, ToolUsageSection, ToolsSection,
};
pub use section::{PromptSection, PromptStability};

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
pub fn default_coding_assembly(tools: Arc<ToolRegistry>, cwd: PathBuf) -> PromptAssembly {
    let mut assembly = PromptAssembly::new();
    assembly.add_static(IdentitySection::new(None));
    assembly.add_static(ToneStyleSection);
    assembly.add_static(TaskGuidanceSection);
    assembly.add_static(SafetySection);
    assembly.add_static(ToolUsageSection);
    assembly.add_static(ToolsSection::new(Arc::clone(&tools)));
    assembly.add_static(ToolPromptsSection::new(Arc::clone(&tools)));
    assembly.add_dynamic(DateSection);
    assembly.add_dynamic(CwdSection::new(cwd));
    assembly
}
