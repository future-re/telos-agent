use std::collections::HashMap;
use std::path::PathBuf;

use crate::memory::format::{MemoryCategory, MemoryEntry, MemoryFormat, MemoryStatus};

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
        let content = MemoryFormat::serialize(&entry);
        std::fs::write(&path, content)?;
        self.index.insert(entry.name.clone(), path);
        self.write_index_md()?;
        Ok(())
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

    /// Search memories by keyword — matches against name, description, tags, and body.
    pub fn search(&self, query: &str) -> Vec<MemoryEntry> {
        let query_lower = query.to_lowercase();
        self.index
            .keys()
            .filter_map(|name| self.read(name))
            .filter(|entry| {
                entry.name.to_lowercase().contains(&query_lower)
                    || entry.description.to_lowercase().contains(&query_lower)
                    || entry.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
                    || entry.body.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// Update the status of a memory entry.
    pub fn update_status(&mut self, name: &str, status: MemoryStatus) -> std::io::Result<()> {
        if let Some(mut entry) = self.read(name) {
            entry.status = status;
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

    /// Get the top N most-used memories for prompt injection.
    pub fn top_by_usage(&self, n: usize) -> Vec<MemoryEntry> {
        let mut entries: Vec<MemoryEntry> =
            self.index.keys().filter_map(|name| self.read(name)).collect();
        entries.sort_by_key(|b| std::cmp::Reverse(b.times_used));
        entries.truncate(n);
        entries
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
}
