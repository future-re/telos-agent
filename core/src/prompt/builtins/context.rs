use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::mcp::manager::McpManager;
use crate::memory::MemoryStatus;
use crate::memory::index::{MemoryQuery, MemorySort, MemoryStore};
use crate::memory::profile::ProfileManager;
use crate::prompt::{PromptSection, PromptStability};
use crate::skills::SkillRegistry;

// ── Date ──────────────────────────────────────────────────

pub struct DateSection;

#[async_trait]
impl PromptSection for DateSection {
    fn name(&self) -> &str {
        "date"
    }
    fn stability(&self) -> PromptStability {
        // A session rarely spans midnight; the date is effectively constant.
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        let date = time::OffsetDateTime::now_utc().date();
        format!("Today's date is {}.", date)
    }
}

// ── CWD ───────────────────────────────────────────────────

pub struct CwdSection {
    cwd: PathBuf,
}

impl CwdSection {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl PromptSection for CwdSection {
    fn name(&self) -> &str {
        "cwd"
    }
    fn stability(&self) -> PromptStability {
        // The working directory doesn't change during a session.
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        format!("Working directory: {}", self.cwd.display())
    }
}

// ── Skills ────────────────────────────────────────────────

pub struct SkillsSection {
    registry: Arc<SkillRegistry>,
}

impl SkillsSection {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl PromptSection for SkillsSection {
    fn name(&self) -> &str {
        "skills"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        self.registry.render_for_prompt()
    }
}

// ── Git Status ────────────────────────────────────────────

pub struct GitStatusSection;

#[async_trait]
impl PromptSection for GitStatusSection {
    fn name(&self) -> &str {
        "git_status"
    }
    fn stability(&self) -> PromptStability {
        // Rendered once at session start; runtime changes appear in tool results.
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        match std::process::Command::new("git").args(["status", "--short"]).output() {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim().is_empty() {
                    "Git: clean working tree.".into()
                } else {
                    format!("## Git Status\n```\n{}\n```", stdout.trim())
                }
            }
            _ => String::new(),
        }
    }
}

// ── Memory ────────────────────────────────────────────────

pub struct MemorySection {
    store: Arc<Mutex<MemoryStore>>,
}

impl MemorySection {
    pub fn new(store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl PromptSection for MemorySection {
    fn name(&self) -> &str {
        "memory"
    }
    fn stability(&self) -> PromptStability {
        // Rendered once at session start; memories are injected as
        // <system-reminder> tags by the runtime when they change.
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        let store = self.store.clone();
        match tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap();
            let mut memories = store.query(MemoryQuery {
                limit: Some(8),
                sort: MemorySort::Relevance,
                ..MemoryQuery::default()
            });
            memories.retain(|entry| entry.status != MemoryStatus::Deprecated);
            memories.truncate(5);
            if memories.is_empty() {
                return String::new();
            }
            let mut lines = vec!["## Relevant Memories".to_string()];
            for entry in &memories {
                lines.push(format!(
                    "- **{}** ({:?}, {:?}): {}",
                    entry.name, entry.category, entry.status, entry.description
                ));
                let preview: String = entry.body.chars().take(200).collect();
                if !preview.is_empty() {
                    lines.push(format!("  {}", preview));
                }
            }
            lines.join("\n")
        })
        .await
        {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("MemorySection::render failed: {e}");
                String::new()
            }
        }
    }
}

// ── Profile ────────────────────────────────────────────────

pub struct ProfileSection {
    profile_manager: Arc<ProfileManager>,
}

impl ProfileSection {
    pub fn new(profile_manager: Arc<ProfileManager>) -> Self {
        Self { profile_manager }
    }
}

#[async_trait]
impl PromptSection for ProfileSection {
    fn name(&self) -> &str {
        "profile"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Dynamic
    }

    async fn render(&self, _ctx: &()) -> String {
        self.profile_manager.render_all()
    }
}

// ── MCP ────────────────────────────────────────────────────

/// Renders a list of tools provided by connected MCP servers.
pub struct McpSection {
    manager: Arc<McpManager>,
}

impl McpSection {
    pub fn new(manager: Arc<McpManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl PromptSection for McpSection {
    fn name(&self) -> &str {
        "mcp"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Dynamic
    }

    async fn render(&self, _ctx: &()) -> String {
        let tools = self.manager.all_tools().await;
        if tools.is_empty() {
            return String::new();
        }
        let mut lines = vec!["## MCP Tools".to_string()];
        for (server_id, tool) in &tools {
            lines.push(format!("- **mcp__{}__{}**: {}", server_id, tool.name, tool.description));
        }
        lines.join("\n")
    }
}
