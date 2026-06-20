use std::collections::HashMap;
use std::path::PathBuf;

use crate::memory::format::{MemoryCategory, MemoryEntry, MemoryFormat, MemoryStatus};

mod maintenance;
mod query;

pub use maintenance::{
    MemoryMaintenanceAction, MemoryMaintenanceActionKind, MemoryMaintenancePolicy,
    MemoryMaintenanceReport,
};
pub use query::{MemoryQuery, MemorySort};

/// Result of inserting or merging a memory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    Created,
    Updated,
}

/// Persistent store for agent memories, organized by category into subdirectories.
/// Maintains a MEMORY.md index file.
pub struct MemoryStore {
    root: PathBuf,
    /// name → full path on disk
    index: HashMap<String, PathBuf>,
}

impl MemoryStore {
    /// Open or create a memory store at the given root directory.
    pub fn new(root: PathBuf) -> Self {
        std::fs::create_dir_all(&root).ok();
        let mut store = Self { root, index: HashMap::new() };
        store.rebuild_index();
        store
    }

    /// Re-scan the root directory and rebuild the in-memory index.
    fn rebuild_index(&mut self) {
        self.index.clear();
        let subdirs = ["scripts", "commands", "patterns", "facts", "workflows"];
        for subdir in subdirs {
            let dir = self.root.join(subdir);
            if !dir.exists() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_none_or(|e| e != "md") {
                        continue;
                    }
                    if let Ok(content) = std::fs::read_to_string(&path)
                        && let Some(mem) = MemoryFormat::parse(&content)
                    {
                        self.index.insert(mem.name.clone(), path);
                    }
                }
            }
        }
    }

    /// Write a memory entry to disk and update the index.
    pub fn write(&mut self, entry: MemoryEntry) -> std::io::Result<()> {
        let filename = sanitize_name(&entry.name);
        let subdir = category_subdir(&entry.category);
        let dir = self.root.join(subdir);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.md", filename));
        if let Some(old_path) = self.index.get(&entry.name)
            && old_path != &path
        {
            match std::fs::remove_file(old_path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        }
        let content = MemoryFormat::serialize(&entry);
        std::fs::write(&path, content)?;
        self.index.insert(entry.name.clone(), path);
        self.write_index_md()?;
        Ok(())
    }

    /// Write a new memory or merge into an existing one with the same name or description.
    pub fn upsert(&mut self, entry: MemoryEntry) -> std::io::Result<UpsertOutcome> {
        let existing_name = if self.index.contains_key(&entry.name) {
            Some(entry.name.clone())
        } else {
            self.index.keys().find_map(|name| {
                let existing = self.read(name)?;
                if existing.description.eq_ignore_ascii_case(&entry.description) {
                    Some(existing.name)
                } else {
                    None
                }
            })
        };

        if let Some(name) = existing_name
            && let Some(mut existing) = self.read(&name)
        {
            existing.description =
                if entry.description.is_empty() { existing.description } else { entry.description };
            existing.category = entry.category;
            existing.tags = merge_strings(existing.tags, entry.tags);
            existing.updated = entry.updated;
            existing.status = entry.status;
            existing.confidence = entry.confidence.or(existing.confidence);
            existing.related = merge_strings(existing.related, entry.related);
            existing.source_session = entry.source_session.or(existing.source_session);
            existing.body = merge_body(&existing.body, &entry.body);
            self.write(existing)?;
            return Ok(UpsertOutcome::Updated);
        }

        self.write(entry)?;
        Ok(UpsertOutcome::Created)
    }

    /// Read a memory entry by name.
    pub fn read(&self, name: &str) -> Option<MemoryEntry> {
        let path = self.index.get(name)?;
        let content = std::fs::read_to_string(path).ok()?;
        MemoryFormat::parse(&content)
    }

    /// Return all memory names.
    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.index.keys().cloned().collect();
        names.sort();
        names
    }

    /// Update the status of a memory entry.
    pub fn update_status(&mut self, name: &str, status: MemoryStatus) -> std::io::Result<()> {
        if let Some(mut entry) = self.read(name) {
            entry.status = status;
            self.write(entry)?;
        }
        Ok(())
    }

    /// Record that a memory was explicitly used.
    pub fn record_use(&mut self, name: &str) -> std::io::Result<()> {
        if let Some(mut entry) = self.read(name) {
            entry.times_used = entry.times_used.saturating_add(1);
            entry.updated = unix_timestamp();
            self.write(entry)?;
        }
        Ok(())
    }

    /// Move a memory to the _archived directory (never delete).
    pub fn archive(&mut self, name: &str) -> std::io::Result<()> {
        if let Some(path) = self.index.remove(name) {
            let archive_dir = self.root.join("_archived");
            std::fs::create_dir_all(&archive_dir)?;
            let dest = archive_dir.join(path.file_name().unwrap());
            std::fs::rename(&path, &dest)?;
            self.write_index_md()?;
        }
        Ok(())
    }

    /// Write the MEMORY.md index file.
    fn write_index_md(&self) -> std::io::Result<()> {
        let mut lines = Vec::new();
        let mut names: Vec<&String> = self.index.keys().collect();
        names.sort();
        for name in names {
            if let Some(entry) = self.read(name) {
                let fname = sanitize_name(name);
                let subdir = category_subdir(&entry.category);
                lines.push(format!(
                    "- [{}]({}/{}.md) — {}",
                    entry.name, subdir, fname, entry.description
                ));
            }
        }
        std::fs::write(self.root.join("MEMORY.md"), lines.join("\n"))
    }
}

/// Map category to storage subdirectory.
fn category_subdir(cat: &MemoryCategory) -> &'static str {
    match cat {
        MemoryCategory::Script => "scripts",
        MemoryCategory::Command => "commands",
        MemoryCategory::Pattern => "patterns",
        MemoryCategory::Fact => "facts",
        MemoryCategory::Workflow => "workflows",
    }
}

/// Sanitize a name for use as a filename.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .to_lowercase()
}

fn merge_strings(mut existing: Vec<String>, incoming: Vec<String>) -> Vec<String> {
    for item in incoming {
        if !existing.iter().any(|e| e.eq_ignore_ascii_case(&item)) {
            existing.push(item);
        }
    }
    existing
}

fn merge_body(existing: &str, incoming: &str) -> String {
    if incoming.trim().is_empty() || existing.contains(incoming.trim()) {
        return existing.to_string();
    }
    if existing.trim().is_empty() {
        return incoming.to_string();
    }
    format!("{}\n\n---\n\n{}", existing.trim_end(), incoming.trim_start())
}

pub fn unix_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::format::{MemoryCategory, MemoryEntry, MemoryStatus};

    fn test_entry(name: &str, desc: &str, cat: MemoryCategory) -> MemoryEntry {
        MemoryEntry {
            name: name.into(),
            description: desc.into(),
            category: cat,
            tags: vec!["test".into()],
            created: "2026-06-18".into(),
            updated: "2026-06-18".into(),
            status: MemoryStatus::Working,
            times_used: 1,
            confidence: None,
            related: vec![],
            source_session: None,
            body: "Test body".into(),
        }
    }

    #[test]
    fn write_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        store.write(test_entry("test-mem", "A test memory", MemoryCategory::Fact)).unwrap();
        let entry = store.read("test-mem").unwrap();
        assert_eq!(entry.name, "test-mem");
        assert_eq!(entry.category, MemoryCategory::Fact);
    }

    #[test]
    fn search_finds_by_tag() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        let mut entry = test_entry("deploy-script", "Deploy script", MemoryCategory::Script);
        entry.tags = vec!["deploy".into(), "staging".into()];
        store.write(entry).unwrap();

        let results = store.search("staging");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "deploy-script");
    }

    #[test]
    fn list_returns_sorted_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        store.write(test_entry("b", "B", MemoryCategory::Fact)).unwrap();
        store.write(test_entry("a", "A", MemoryCategory::Fact)).unwrap();

        let names = store.list();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn archive_moves_to_archived_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        store.write(test_entry("old-mem", "Old", MemoryCategory::Fact)).unwrap();
        store.archive("old-mem").unwrap();

        assert!(store.read("old-mem").is_none());
        assert!(dir.path().join("_archived").exists());
    }

    #[test]
    fn update_status_changes_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        store.write(test_entry("flaky-script", "Flaky", MemoryCategory::Script)).unwrap();
        store.update_status("flaky-script", MemoryStatus::NeedsFix).unwrap();

        let entry = store.read("flaky-script").unwrap();
        assert_eq!(entry.status, MemoryStatus::NeedsFix);
    }

    #[test]
    fn top_by_usage_returns_most_used() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        let mut high = test_entry("high-usage", "High", MemoryCategory::Fact);
        high.times_used = 10;
        let mut low = test_entry("low-usage", "Low", MemoryCategory::Fact);
        low.times_used = 1;
        store.write(high).unwrap();
        store.write(low).unwrap();

        let top = store.top_by_usage(1);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].name, "high-usage");
    }

    #[test]
    fn upsert_updates_existing_memory_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        store.write(test_entry("same", "Old", MemoryCategory::Fact)).unwrap();
        let mut replacement = test_entry("same", "New", MemoryCategory::Command);
        replacement.tags = vec!["new".into()];
        replacement.body = "New body".into();

        let outcome = store.upsert(replacement).unwrap();
        assert_eq!(outcome, UpsertOutcome::Updated);

        let entry = store.read("same").unwrap();
        assert_eq!(entry.description, "New");
        assert_eq!(entry.category, MemoryCategory::Command);
        assert!(entry.tags.contains(&"new".to_string()));
        assert!(entry.body.contains("Test body"));
        assert!(entry.body.contains("New body"));
    }

    #[test]
    fn category_change_survives_reopening_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        store.write(test_entry("same", "Old", MemoryCategory::Fact)).unwrap();
        let mut replacement = test_entry("same", "New", MemoryCategory::Command);
        replacement.body = "New body".into();
        store.upsert(replacement).unwrap();

        let reopened = MemoryStore::new(dir.path().to_path_buf());
        let entry = reopened.read("same").unwrap();
        assert_eq!(entry.category, MemoryCategory::Command);
        assert_eq!(entry.description, "New");
        assert!(entry.body.contains("New body"));
    }

    #[test]
    fn query_filters_status_and_tags() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        let mut fix = test_entry("fix", "Fix", MemoryCategory::Fact);
        fix.status = MemoryStatus::NeedsFix;
        fix.tags = vec!["error".into()];
        store.write(fix).unwrap();
        store.write(test_entry("ok", "Ok", MemoryCategory::Fact)).unwrap();

        let results = store.query(MemoryQuery {
            status: Some(MemoryStatus::NeedsFix),
            tags: vec!["error".into()],
            include_body: false,
            ..MemoryQuery::default()
        });

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "fix");
        assert!(results[0].body.is_empty());
    }

    #[test]
    fn record_use_increments_times_used() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        store.write(test_entry("used", "Used", MemoryCategory::Fact)).unwrap();
        store.record_use("used").unwrap();

        let entry = store.read("used").unwrap();
        assert_eq!(entry.times_used, 2);
    }

    #[test]
    fn maintenance_report_archives_deprecated_and_excess_auto_learned_commands() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        let mut old_auto = test_entry("old-auto", "Old auto command", MemoryCategory::Command);
        old_auto.tags = vec!["auto-learned".into(), "bash".into()];
        old_auto.updated = "100".into();
        old_auto.times_used = 0;

        let mut new_auto = test_entry("new-auto", "New auto command", MemoryCategory::Command);
        new_auto.tags = vec!["auto-learned".into(), "bash".into()];
        new_auto.updated = "200".into();
        new_auto.times_used = 0;

        let mut used_auto = test_entry("used-auto", "Used auto command", MemoryCategory::Command);
        used_auto.tags = vec!["auto-learned".into(), "bash".into()];
        used_auto.updated = "50".into();
        used_auto.times_used = 3;

        let mut deprecated = test_entry("deprecated", "Deprecated fact", MemoryCategory::Fact);
        deprecated.status = MemoryStatus::Deprecated;

        store.write(old_auto).unwrap();
        store.write(new_auto).unwrap();
        store.write(used_auto).unwrap();
        store.write(deprecated).unwrap();
        store.write(test_entry("manual", "Manual fact", MemoryCategory::Fact)).unwrap();

        let report = store.maintenance_report(&MemoryMaintenancePolicy {
            max_auto_learned_commands: Some(2),
            archive_deprecated: true,
            ..MemoryMaintenancePolicy::default()
        });

        let names: Vec<&str> = report.actions.iter().map(|action| action.name.as_str()).collect();
        assert_eq!(names, vec!["deprecated", "old-auto"]);
        assert!(report.actions.iter().any(|action| action.reason.contains("deprecated")));
        assert!(report.actions.iter().any(|action| action.reason.contains("auto-learned")));
    }

    #[test]
    fn maintenance_apply_moves_candidates_to_archive() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());

        let mut old_auto = test_entry("old-auto", "Old auto command", MemoryCategory::Command);
        old_auto.tags = vec!["auto-learned".into(), "bash".into()];
        old_auto.updated = "100".into();
        old_auto.times_used = 0;

        let mut new_auto = test_entry("new-auto", "New auto command", MemoryCategory::Command);
        new_auto.tags = vec!["auto-learned".into(), "bash".into()];
        new_auto.updated = "200".into();
        new_auto.times_used = 0;

        store.write(old_auto).unwrap();
        store.write(new_auto).unwrap();

        let report = store
            .apply_maintenance(&MemoryMaintenancePolicy {
                max_auto_learned_commands: Some(1),
                archive_deprecated: false,
                ..MemoryMaintenancePolicy::default()
            })
            .unwrap();

        assert!(report.applied);
        assert_eq!(report.archived_count, 1);
        assert!(store.read("old-auto").is_none());
        assert!(store.read("new-auto").is_some());
        assert!(dir.path().join("_archived").join("old-auto.md").exists());
    }
}
