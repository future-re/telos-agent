use std::sync::{Arc, Mutex};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crate::knowledge::memory::MemoryStore;
use crate::model::message::SystemReminder;

pub struct MemoryInjector {
    store: Arc<Mutex<MemoryStore>>,
    max_memories: usize,
    min_relevance: f64,
}

pub struct MemoryInjection {
    pub reminder: SystemReminder,
    pub fingerprint: u64,
}

impl MemoryInjector {
    pub fn new(store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { store, max_memories: 5, min_relevance: 0.10 }
    }

    pub fn with_max_memories(mut self, max: usize) -> Self {
        self.max_memories = max;
        self
    }

    pub fn with_min_relevance(mut self, threshold: f64) -> Self {
        self.min_relevance = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn inject_for_query(&self, query: &str) -> Option<MemoryInjection> {
        let store = self.store.lock().ok()?;
        let memories = store.search_relevant(query, self.max_memories, self.min_relevance);
        if memories.is_empty() {
            return None;
        }

        let mut lines = vec!["## Relevant Memories".to_string(), String::new()];
        for entry in &memories {
            let status_tag = match entry.status {
                crate::knowledge::memory::MemoryStatus::NeedsFix => " [needs_fix]",
                _ => "",
            };
            lines.push(format!(
                "- **{}** ({:?}{}): {}",
                entry.name, entry.category, status_tag, entry.description,
            ));
            let preview: String = entry.body.chars().take(80).collect();
            if !preview.is_empty() {
                lines.push(format!("  {}", preview));
            }
        }
        let content = lines.join("\n");
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        Some(MemoryInjection {
            reminder: SystemReminder::MemoryInjection { content },
            fingerprint: hasher.finish(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::memory::format::{MemoryCategory, MemoryEntry, MemoryStatus};

    fn test_entry(name: &str, body: &str) -> MemoryEntry {
        MemoryEntry {
            name: name.into(),
            description: "Relevant memory".into(),
            category: MemoryCategory::Fact,
            tags: vec!["cache".into(), "prompt".into()],
            created: "2026-06-23".into(),
            updated: "2026-06-23".into(),
            status: MemoryStatus::Working,
            times_used: 3,
            confidence: None,
            related: vec![],
            source_session: None,
            body: body.into(),
        }
    }

    #[test]
    fn identical_relevant_memories_produce_stable_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .write(test_entry("cache-hit-guide", "Prompt cache hit guidance for desktop runtime."))
            .unwrap();
        let store = Arc::new(Mutex::new(store));
        let injector = MemoryInjector::new(Arc::clone(&store));

        let first = injector
            .inject_for_query("How do I improve prompt cache hit rate in desktop runtime?")
            .expect("first injection should exist");
        let second = injector
            .inject_for_query("How do I improve prompt cache hit rate in desktop runtime?")
            .expect("second injection should exist");

        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.reminder, second.reminder);
    }

    #[test]
    fn injection_preview_is_truncated_to_eighty_chars() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let long_body = "a".repeat(140);
        store.write(test_entry("long-memory", &long_body)).unwrap();
        let store = Arc::new(Mutex::new(store));
        let injector = MemoryInjector::new(store);

        let injection =
            injector.inject_for_query("long memory prompt cache").expect("injection should exist");

        let rendered = injection.reminder.render();
        assert!(rendered.contains(&"a".repeat(80)));
        assert!(!rendered.contains(&"a".repeat(81)));
    }
}
