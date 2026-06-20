//! Built-in tools: shell, file read/write/edit, glob, grep.
//!
//! Each tool gates write access through [`PermissionDecision::Ask`] so the
//! host (typically a human approval prompt) keeps the final say. Read-only
//! tools are marked concurrency-safe so they can run in parallel batches.

use std::sync::{Arc, Mutex};

use crate::skills::SkillRegistry;
use crate::tasks::TaskManager;
use crate::tool::ToolRegistry;

mod ask_user_question;
mod browser;
mod code_index;
mod domain_filter;
mod file_edit;
mod file_read;
mod file_write;
mod glob;
mod grep;
mod memory;
mod shared;
mod shell;
mod skill;
mod tasks;
mod web_fetch;
mod web_search;

pub use ask_user_question::AskUserQuestionTool;
pub use browser::{
    BrowserBackTool, BrowserClickTool, BrowserCloseTool, BrowserFindUrlTool, BrowserManager,
    BrowserNavigateTool, BrowserScreenshotTool, BrowserScrollTool, BrowserSelectTool,
    BrowserStartTool, BrowserStateTool, BrowserTypeTool,
};
pub use code_index::{CodeContextTool, CodeIndexRefreshTool, CodeSearchTool};
pub use file_edit::FileEditTool;
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use memory::{
    MemoryEditTool, MemoryGrepTool, MemoryMaintenanceTool, MemoryReadTool, MemoryStatusTool,
    MemoryWriteTool,
};
pub use shell::ShellTool;
pub use skill::SkillTool;
pub use tasks::{TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool};
pub use web_fetch::WebFetchTool;
pub use web_search::WebSearchTool;

/// Register every built-in tool with the supplied registry.
pub fn register_core_tools(registry: &mut ToolRegistry) {
    let browser_manager = BrowserManager::new();
    registry.register(ShellTool);
    registry.register(FileReadTool);
    registry.register(FileWriteTool);
    registry.register(FileEditTool);
    registry.register(GlobTool);
    registry.register(GrepTool);
    registry.register(CodeSearchTool);
    registry.register(CodeContextTool);
    registry.register(CodeIndexRefreshTool);
    registry.register(WebFetchTool::new());
    registry.register(WebSearchTool);
    registry.register(BrowserStartTool::new(browser_manager.clone()));
    registry.register(BrowserNavigateTool::new(browser_manager.clone()));
    registry.register(BrowserStateTool::new(browser_manager.clone()));
    registry.register(BrowserClickTool::new(browser_manager.clone()));
    registry.register(BrowserTypeTool::new(browser_manager.clone()));
    registry.register(BrowserSelectTool::new(browser_manager.clone()));
    registry.register(BrowserScrollTool::new(browser_manager.clone()));
    registry.register(BrowserBackTool::new(browser_manager.clone()));
    registry.register(BrowserScreenshotTool::new(browser_manager.clone()));
    registry.register(BrowserCloseTool::new(browser_manager));
    registry.register(BrowserFindUrlTool);
}

/// Register task tracking tools with the supplied registry.
pub fn register_task_tools(registry: &mut ToolRegistry, task_manager: Arc<TaskManager>) {
    registry.register(TaskCreateTool::new(task_manager.clone()));
    registry.register(TaskGetTool::new(task_manager.clone()));
    registry.register(TaskListTool::new(task_manager.clone()));
    registry.register(TaskUpdateTool::new(task_manager));
}

/// Register memory tools with the supplied registry.
pub fn register_memory_tools(
    registry: &mut ToolRegistry,
    store: Arc<Mutex<crate::memory::MemoryStore>>,
) {
    registry.register(MemoryReadTool::new(store.clone()));
    registry.register(MemoryWriteTool::new(store.clone()));
    registry.register(MemoryGrepTool::new(store.clone()));
    registry.register(MemoryEditTool::new(store.clone()));
    registry.register(MemoryStatusTool::new(store.clone()));
    registry.register(MemoryMaintenanceTool::new(store));
}

/// Register the Skill tool if a skill registry is available.
pub fn register_skill_tool(registry: &mut ToolRegistry, skill_registry: Arc<SkillRegistry>) {
    registry.register(SkillTool::new(skill_registry));
}

// Re-export shared helpers that other crate modules use directly.
pub(crate) use shared::{
    canonicalize_within_cwd, display_relative, ensure_file_was_read_and_unchanged,
    modified_timestamp_ms, optional_bool, optional_usize_any, required_string, required_string_any,
    resolve_workspace_path,
};
