//! Built-in tools: shell, file read/write/edit, glob, grep.
//!
//! Each tool gates write access through [`PermissionDecision::Ask`] so the
//! host (typically a human approval prompt) keeps the final say. Read-only
//! tools are marked concurrency-safe so they can run in parallel batches.

use crate::tool::ToolRegistry;

mod file_edit;
mod file_read;
mod file_write;
mod glob;
mod grep;
mod shared;
mod shell;

pub use file_edit::FileEditTool;
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use shell::ShellTool;

/// Register every built-in tool with the supplied registry.
pub fn register_core_tools(registry: &mut ToolRegistry) {
    registry.register(ShellTool);
    registry.register(FileReadTool);
    registry.register(FileWriteTool);
    registry.register(FileEditTool);
    registry.register(GlobTool);
    registry.register(GrepTool);
}

// Re-export shared helpers that other crate modules use directly.
pub(crate) use shared::{
    canonicalize_within_cwd, display_relative, ensure_file_was_read_and_unchanged, is_within_cwd,
    modified_timestamp_ms, optional_bool, optional_usize_any, required_string, required_string_any,
    resolve_workspace_path,
};
