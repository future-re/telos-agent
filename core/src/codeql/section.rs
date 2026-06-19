//! [`CodeqlSection`] — a [`PromptSection`] that injects active CodeQL findings
//! (those with `status = NeedsFix` and tagged `"codeql"`) into the system prompt
//! so the agent is always aware of open security and quality issues.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::memory::format::MemoryStatus;
use crate::memory::index::{MemoryQuery, MemorySort, MemoryStore};
use crate::prompt::{PromptSection, PromptStability};

/// Injects a summary of open CodeQL findings into the system prompt.
///
/// The section is **dynamic** — it re-reads from the memory store on every
/// prompt build so it always reflects the latest state.
pub struct CodeqlSection {
    store: Arc<Mutex<MemoryStore>>,
}

impl CodeqlSection {
    pub fn new(store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl PromptSection for CodeqlSection {
    fn name(&self) -> &str {
        "codeql"
    }

    fn stability(&self) -> PromptStability {
        PromptStability::Dynamic
    }

    async fn render(&self, _ctx: &()) -> String {
        let store = self.store.clone();
        match tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap();
            let mut findings = store.query(MemoryQuery {
                tags: vec!["codeql".into()],
                status: Some(MemoryStatus::NeedsFix),
                limit: Some(8),
                sort: MemorySort::RecentlyUpdated,
                include_body: false,
            });

            if findings.is_empty() {
                return String::new();
            }

            // Sort errors before warnings.
            findings.sort_by_key(|f| if f.tags.iter().any(|t| t == "error") { 0 } else { 1 });
            findings.truncate(5);

            let mut lines = vec!["## CodeQL Findings".to_string()];
            for entry in &findings {
                let severity =
                    if entry.tags.iter().any(|t| t == "error") { "error" } else { "warning" };
                lines.push(format!(
                    "- **{}** ({severity}): {} — `{}`",
                    entry.name.trim_start_matches("codeql-"),
                    entry.description,
                    entry.body.lines().nth(2).unwrap_or("").trim_start_matches("**Location**: "),
                ));
            }
            lines.join("\n")
        })
        .await
        {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("CodeqlSection::render failed: {e}");
                String::new()
            }
        }
    }
}
