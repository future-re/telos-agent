use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::memory::index::MemoryStore;
use crate::memory::profile::ProfileManager;
use crate::prompt::{PromptSection, PromptStability};
use crate::skills::SkillRegistry;
use crate::tool::ToolRegistry;

// ── Identity ──────────────────────────────────────────────

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
        let mut parts = vec![
            "You are tiny-agent, a CLI coding assistant.".to_string(),
            "You have access to tools for reading/writing files, running shell commands, searching code, and more.".to_string(),
        ];
        if let Some(base) = &self.base {
            parts.push(base.clone());
        }
        parts.join("\n")
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

// ── Date ──────────────────────────────────────────────────

pub struct DateSection;

#[async_trait]
impl PromptSection for DateSection {
    fn name(&self) -> &str {
        "date"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Dynamic
    }

    async fn render(&self, _ctx: &()) -> String {
        // Approximate date calculation without chrono dependency
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let secs = now.as_secs();
        // Days since Jan 1, 2024 (epoch 1704067200)
        let days_since_2024 = secs.saturating_sub(1704067200) / 86400;
        let year = 2024 + (days_since_2024 / 365);
        let day_of_year = days_since_2024 % 365;
        let month_lengths = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut remaining = day_of_year;
        let mut month = 1;
        for &ml in &month_lengths {
            if remaining < ml {
                break;
            }
            remaining -= ml;
            month += 1;
        }
        let day = remaining + 1;
        format!("Today's date is {year}-{month:02}-{day:02}.")
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
        PromptStability::Dynamic
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
        PromptStability::Dynamic
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
        PromptStability::Dynamic
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
        PromptStability::Dynamic
    }

    async fn render(&self, _ctx: &()) -> String {
        let store = self.store.lock().unwrap();
        let top = store.top_by_usage(5);
        if top.is_empty() {
            return String::new();
        }
        let mut lines = vec!["## Relevant Memories".to_string()];
        for entry in &top {
            lines.push(format!(
                "- **{}** ({:?}): {}",
                entry.name, entry.category, entry.description
            ));
            // Include first 200 chars of body as context
            let preview: String = entry.body.chars().take(200).collect();
            if !preview.is_empty() {
                lines.push(format!("  {}", preview));
            }
        }
        lines.join("\n")
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
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        self.profile_manager.render_all()
    }
}
