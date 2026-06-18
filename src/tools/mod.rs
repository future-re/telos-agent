//! Built-in tools: shell, file read/write/edit, glob, grep.
//!
//! Each tool gates write access through [`PermissionDecision::Ask`] so the
//! host (typically a human approval prompt) keeps the final say. Read-only
//! tools are marked concurrency-safe so they can run in parallel batches.

use std::sync::Arc;

use crate::tasks::TaskManager;
use crate::tool::ToolRegistry;

mod ask_user_question;
mod file_edit;
mod file_read;
mod file_write;
mod glob;
mod grep;
mod shared;
mod shell;
mod skill;
mod web_fetch;
mod web_search;

pub use crate::tasks::tool::{TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool};
pub use ask_user_question::AskUserQuestionTool;
pub use file_edit::FileEditTool;
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use shell::ShellTool;
pub use skill::SkillTool;
pub use web_fetch::WebFetchTool;
pub use web_search::WebSearchTool;

/// Register every built-in tool with the supplied registry.
pub fn register_core_tools(registry: &mut ToolRegistry) {
    registry.register(ShellTool);
    registry.register(FileReadTool);
    registry.register(FileWriteTool);
    registry.register(FileEditTool);
    registry.register(GlobTool);
    registry.register(GrepTool);
    registry.register(WebFetchTool::new());
    registry.register(WebSearchTool);
}

/// Register task tracking tools with the supplied registry.
pub fn register_task_tools(registry: &mut ToolRegistry, task_manager: Arc<TaskManager>) {
    registry.register(TaskCreateTool::new(task_manager.clone()));
    registry.register(TaskGetTool::new(task_manager.clone()));
    registry.register(TaskListTool::new(task_manager.clone()));
    registry.register(TaskUpdateTool::new(task_manager));
}

// Re-export shared helpers that other crate modules use directly.
pub(crate) use shared::{
    canonicalize_within_cwd, display_relative, ensure_file_was_read_and_unchanged,
    modified_timestamp_ms, optional_bool, optional_usize_any, required_string, required_string_any,
    resolve_workspace_path,
};
