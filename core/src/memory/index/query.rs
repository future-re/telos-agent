use std::cmp::Ordering;

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
    /// Query memories by metadata. Uses the in-memory cache — no disk I/O.
    pub fn query(&self, query: MemoryQuery) -> Vec<MemoryEntry> {
        let tags: Vec<String> = query.tags.iter().map(|tag| tag.to_lowercase()).collect();
        let mut entries: Vec<MemoryEntry> = self
            .cache
            .values()
            .filter(|entry| {
                query.status.as_ref().is_none_or(|status| &entry.status == status)
                    && tags.iter().all(|tag| entry.tags.iter().any(|t| t.to_lowercase() == *tag))
            })
            .cloned()
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

    /// Search memories by keyword — matches name, description, tags, and body.
    /// Uses the in-memory cache — no disk I/O.
    pub fn search(&self, query: &str) -> Vec<MemoryEntry> {
        let query_lower = query.to_lowercase();
        self.cache
            .values()
            .filter(|entry| {
                entry.name.to_lowercase().contains(&query_lower)
                    || entry.description.to_lowercase().contains(&query_lower)
                    || entry.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
                    || entry.body.to_lowercase().contains(&query_lower)
            })
            .cloned()
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

    /// Score cached memories against a user query and return the top-K by
    /// keyword-overlap relevance. Non-deprecated entries only.
    ///
    /// This is the primary method for dynamic memory injection — unlike
    /// `query()` + `MemorySort::Relevance` (which sorts by metadata), this
    /// uses the actual content of the user's prompt to find semantically
    /// relevant memories via simple token overlap.
    pub fn search_relevant(&self, query: &str, limit: usize) -> Vec<MemoryEntry> {
        let query_tokens = tokenize(query);
        if query_tokens.is_empty() || self.cache.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<(f64, &MemoryEntry)> = self
            .cache
            .values()
            .filter(|e| e.status != MemoryStatus::Deprecated)
            .map(|entry| {
                let raw_score = compute_relevance(&query_tokens, entry);
                (raw_score, entry)
            })
            .collect();

        // Sort descending by score, stable for deterministic output.
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal).then_with(|| a.1.name.cmp(&b.1.name))
        });
        scored.truncate(limit);
        scored.into_iter().map(|(_, e)| e.clone()).collect()
    }
}

pub(super) fn status_rank(status: &MemoryStatus) -> u8 {
    match status {
        MemoryStatus::NeedsFix => 0,
        MemoryStatus::Working => 1,
        MemoryStatus::Deprecated => 2,
    }
}

// ── Relevance scoring ───────────────────────────────────────

/// Tokenize text into lowercase words, skipping single-char and empty tokens.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|s| s.len() > 1)
        .map(String::from)
        .collect()
}

/// Tokenize an entry's searchable fields into a single token list.
#[allow(dead_code)]
fn tokenize_entry(entry: &MemoryEntry) -> Vec<String> {
    let mut tokens = tokenize(&entry.name);
    tokens.extend(tokenize(&entry.description));
    for tag in &entry.tags {
        tokens.extend(tokenize(tag));
    }
    tokens
}

/// Fraction of `query_tokens` that appear (partially) in `entry_tokens`.
fn token_overlap(query_tokens: &[String], entry_tokens: &[String]) -> f64 {
    if query_tokens.is_empty() || entry_tokens.is_empty() {
        return 0.0;
    }
    let matches = query_tokens
        .iter()
        .filter(|qt| {
            entry_tokens.iter().any(|et| et.contains(qt.as_str()) || qt.contains(et.as_str()))
        })
        .count();
    matches as f64 / query_tokens.len() as f64
}

/// Score a single memory entry against the tokenized user query.
///
/// Weights are tuned for coding-agent use: name and description are
/// the strongest signals (they're written for retrieval); tags add
/// categorical relevance; body is a weaker signal because it's noisy.
fn compute_relevance(query_tokens: &[String], entry: &MemoryEntry) -> f64 {
    let name_tokens = tokenize(&entry.name);
    let desc_tokens = tokenize(&entry.description);
    let tag_tokens: Vec<String> = entry.tags.iter().flat_map(|t| tokenize(t)).collect();
    let body_tokens = tokenize(&entry.body);

    let name_score = token_overlap(query_tokens, &name_tokens) * 0.30;
    let desc_score = token_overlap(query_tokens, &desc_tokens) * 0.25;
    let tag_score = token_overlap(query_tokens, &tag_tokens) * 0.20;
    let body_score = token_overlap(query_tokens, &body_tokens) * 0.15;

    let status_mult = match entry.status {
        MemoryStatus::Working => 1.0,
        MemoryStatus::NeedsFix => 0.5,
        MemoryStatus::Deprecated => 0.0,
    };

    let usage_boost = ((entry.times_used as f64 + 1.0).ln() / 10.0).min(0.10);

    (name_score + desc_score + tag_score + body_score) * status_mult + usage_boost
}
