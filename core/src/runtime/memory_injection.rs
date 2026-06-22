use std::sync::{Arc, Mutex};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crate::memory::MemoryStore;
use crate::message::SystemReminder;

/// Scores and injects relevant memories into the turn as system reminders.
///
/// At the start of each turn, the injector scores all cached memories
/// against the user's current input using keyword-overlap relevance
/// (see [`MemoryStore::search_relevant`]). The top results are formatted
/// as markdown and wrapped in a [`SystemReminder::MemoryInjection`].
pub struct MemoryInjector {
    store: Arc<Mutex<MemoryStore>>,
    max_memories: usize,
}

pub struct MemoryInjection {
    pub reminder: SystemReminder,
    pub fingerprint: u64,
}

impl MemoryInjector {
    pub fn new(store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { store, max_memories: 8 }
    }

    /// Override the default maximum number of injected memories (8).
    pub fn with_max_memories(mut self, max: usize) -> Self {
        self.max_memories = max;
        self
    }

    /// Score cached memories against `query` and return a
    /// [`SystemReminder::MemoryInjection`] containing the top-K results.
    ///
    /// Returns `None` when the store has no relevant memories (empty store,
    /// no matches, or all deprecated).
    pub fn inject_for_query(&self, query: &str) -> Option<MemoryInjection> {
        let store = self.store.lock().ok()?;
        let memories = store.search_relevant(query, self.max_memories);
        if memories.is_empty() {
            return None;
        }

        let mut lines = vec!["## Relevant Memories".to_string(), String::new()];
        for entry in &memories {
            let status_tag = match entry.status {
                crate::memory::MemoryStatus::NeedsFix => " [needs_fix]",
                _ => "",
            };
            lines.push(format!(
                "- **{}** ({:?}{}): {}",
                entry.name, entry.category, status_tag, entry.description,
            ));
            let preview: String = entry.body.chars().take(96).collect();
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
