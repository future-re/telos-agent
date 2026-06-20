use crate::memory::format::{MemoryEntry, MemoryStatus};
use crate::memory::index::MemoryStore;

/// Sort order for memory queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySort {
    Relevance,
    RecentlyUpdated,
    MostUsed,
}

/// Query options for selecting memories.
#[derive(Debug, Clone)]
pub struct MemoryQuery {
    pub status: Option<MemoryStatus>,
    pub tags: Vec<String>,
    pub limit: Option<usize>,
    pub include_body: bool,
    pub sort: MemorySort,
}

impl Default for MemoryQuery {
    fn default() -> Self {
        Self {
            status: None,
            tags: Vec::new(),
            limit: None,
            include_body: true,
            sort: MemorySort::Relevance,
        }
    }
}

impl MemoryStore {
    /// Query memories by metadata.
    pub fn query(&self, query: MemoryQuery) -> Vec<MemoryEntry> {
        let tags: Vec<String> = query.tags.iter().map(|tag| tag.to_lowercase()).collect();
        let mut entries: Vec<MemoryEntry> = self
            .index
            .keys()
            .filter_map(|name| self.read(name))
            .filter(|entry| {
                query.status.as_ref().is_none_or(|status| &entry.status == status)
                    && tags.iter().all(|tag| entry.tags.iter().any(|t| t.to_lowercase() == *tag))
            })
            .collect();

        match query.sort {
            MemorySort::Relevance => entries.sort_by(|a, b| {
                status_rank(&a.status)
                    .cmp(&status_rank(&b.status))
                    .then_with(|| {
                        std::cmp::Reverse(a.times_used).cmp(&std::cmp::Reverse(b.times_used))
                    })
                    .then_with(|| b.updated.cmp(&a.updated))
                    .then_with(|| a.name.cmp(&b.name))
            }),
            MemorySort::RecentlyUpdated => {
                entries.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.name.cmp(&b.name)));
            }
            MemorySort::MostUsed => entries.sort_by_key(|b| std::cmp::Reverse(b.times_used)),
        }

        if let Some(limit) = query.limit {
            entries.truncate(limit);
        }
        if !query.include_body {
            for entry in &mut entries {
                entry.body.clear();
            }
        }
        entries
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

    /// Get the top N most-used memories for prompt injection.
    pub fn top_by_usage(&self, n: usize) -> Vec<MemoryEntry> {
        self.query(MemoryQuery {
            status: Some(MemoryStatus::Working),
            limit: Some(n),
            sort: MemorySort::MostUsed,
            ..MemoryQuery::default()
        })
    }
}

pub(super) fn status_rank(status: &MemoryStatus) -> u8 {
    match status {
        MemoryStatus::NeedsFix => 0,
        MemoryStatus::Working => 1,
        MemoryStatus::Deprecated => 2,
    }
}
