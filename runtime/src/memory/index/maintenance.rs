use std::collections::HashSet;

use serde::Serialize;

use crate::memory::format::{MemoryCategory, MemoryEntry, MemoryStatus};
use crate::memory::index::MemoryStore;
use crate::memory::index::query::status_rank;

/// Conservative cleanup knobs for persistent memory.
#[derive(Debug, Clone)]
pub struct MemoryMaintenancePolicy {
    /// Keep at most this many active auto-learned command memories.
    pub max_auto_learned_commands: Option<usize>,
    /// Keep at most this many active memories overall.
    pub max_active_entries: Option<usize>,
    /// Move deprecated memories out of the active index.
    pub archive_deprecated: bool,
}

impl Default for MemoryMaintenancePolicy {
    fn default() -> Self {
        Self {
            max_auto_learned_commands: Some(20),
            max_active_entries: None,
            archive_deprecated: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMaintenanceActionKind {
    Archive,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MemoryMaintenanceAction {
    pub name: String,
    pub action: MemoryMaintenanceActionKind,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MemoryMaintenanceReport {
    pub applied: bool,
    pub active_before: usize,
    pub archived_count: usize,
    pub actions: Vec<MemoryMaintenanceAction>,
}

impl MemoryStore {
    /// Build a dry-run report of memories that would be archived by the policy.
    pub fn maintenance_report(&self, policy: &MemoryMaintenancePolicy) -> MemoryMaintenanceReport {
        let mut entries: Vec<MemoryEntry> =
            self.index.keys().filter_map(|name| self.read(name)).collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let mut actions = Vec::new();
        let mut selected = HashSet::new();

        if policy.archive_deprecated {
            for entry in entries.iter().filter(|entry| entry.status == MemoryStatus::Deprecated) {
                push_archive_action(&mut actions, &mut selected, entry, "status is deprecated");
            }
        }

        if let Some(max) = policy.max_auto_learned_commands {
            let mut commands: Vec<&MemoryEntry> =
                entries.iter().filter(|entry| is_auto_learned_command(entry)).collect();
            if commands.len() > max {
                commands.sort_by(|a, b| {
                    a.times_used
                        .cmp(&b.times_used)
                        .then_with(|| {
                            updated_sort_value(&a.updated).cmp(&updated_sort_value(&b.updated))
                        })
                        .then_with(|| a.updated.cmp(&b.updated))
                        .then_with(|| a.name.cmp(&b.name))
                });
                let overflow = commands.len().saturating_sub(max);
                for entry in commands.into_iter().take(overflow) {
                    push_archive_action(
                        &mut actions,
                        &mut selected,
                        entry,
                        "exceeds auto-learned command retention limit",
                    );
                }
            }
        }

        if let Some(max) = policy.max_active_entries {
            let remaining_active = entries.len().saturating_sub(selected.len());
            if remaining_active > max {
                let mut candidates: Vec<&MemoryEntry> =
                    entries.iter().filter(|entry| !selected.contains(&entry.name)).collect();
                candidates.sort_by(|a, b| {
                    status_rank(&b.status)
                        .cmp(&status_rank(&a.status))
                        .then_with(|| a.times_used.cmp(&b.times_used))
                        .then_with(|| {
                            updated_sort_value(&a.updated).cmp(&updated_sort_value(&b.updated))
                        })
                        .then_with(|| a.updated.cmp(&b.updated))
                        .then_with(|| a.name.cmp(&b.name))
                });
                for entry in candidates.into_iter().take(remaining_active - max) {
                    push_archive_action(
                        &mut actions,
                        &mut selected,
                        entry,
                        "exceeds active memory retention limit",
                    );
                }
            }
        }

        MemoryMaintenanceReport {
            applied: false,
            active_before: entries.len(),
            archived_count: 0,
            actions,
        }
    }

    /// Apply a maintenance policy by archiving the report's candidate memories.
    pub fn apply_maintenance(
        &mut self,
        policy: &MemoryMaintenancePolicy,
    ) -> std::io::Result<MemoryMaintenanceReport> {
        let mut report = self.maintenance_report(policy);
        let actions = report.actions.clone();
        let mut archived = 0;
        for action in actions {
            match action.action {
                MemoryMaintenanceActionKind::Archive => {
                    self.archive(&action.name)?;
                    archived += 1;
                }
            }
        }
        report.applied = true;
        report.archived_count = archived;
        Ok(report)
    }
}

fn push_archive_action(
    actions: &mut Vec<MemoryMaintenanceAction>,
    selected: &mut HashSet<String>,
    entry: &MemoryEntry,
    reason: &str,
) {
    if selected.insert(entry.name.clone()) {
        actions.push(MemoryMaintenanceAction {
            name: entry.name.clone(),
            action: MemoryMaintenanceActionKind::Archive,
            reason: reason.to_string(),
        });
    }
}

fn is_auto_learned_command(entry: &MemoryEntry) -> bool {
    entry.category == MemoryCategory::Command
        && entry.status == MemoryStatus::Working
        && entry.tags.iter().any(|tag| tag.eq_ignore_ascii_case("auto-learned"))
}

fn updated_sort_value(value: &str) -> u64 {
    value.parse().unwrap_or(0)
}
