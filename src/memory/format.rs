use serde::{Deserialize, Serialize};

/// Category of a memory entry — determines storage subdirectory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    Script,
    Command,
    Pattern,
    Fact,
    Workflow,
}

/// Lifecycle status of a memory entry.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    #[default]
    Working,
    #[serde(rename = "needs_fix")]
    NeedsFix,
    Deprecated,
}

/// A single memory entry — stored as a markdown file with YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub name: String,
    pub description: String,
    pub category: MemoryCategory,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created: String,
    pub updated: String,
    #[serde(default)]
    pub status: MemoryStatus,
    #[serde(default)]
    pub times_used: u32,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub related: Vec<String>,
    #[serde(default)]
    pub source_session: Option<String>,
    /// Body markdown — NOT serialized in YAML, stored after the frontmatter.
    #[serde(skip)]
    pub body: String,
}

/// Parses and serializes memory entries in YAML frontmatter + markdown format.
pub struct MemoryFormat;

impl MemoryFormat {
    /// Parse a memory file content into a MemoryEntry.
    /// Returns None if frontmatter is missing, malformed, or missing required fields.
    pub fn parse(content: &str) -> Option<MemoryEntry> {
        let content = content.trim();
        let rest = content.strip_prefix("---")?;
        let (frontmatter, body) = rest.split_once("\n---")?;
        let body = body.trim().to_string();
        let mut entry: MemoryEntry = serde_yaml::from_str(frontmatter).ok()?;
        entry.body = body;
        Some(entry)
    }

    /// Serialize a MemoryEntry to a string suitable for writing to a file.
    pub fn serialize(entry: &MemoryEntry) -> String {
        let frontmatter = serde_yaml::to_string(entry).unwrap_or_default();
        format!("---\n{}---\n\n{}", frontmatter, entry.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_memory_entry() {
        let content = r#"---
name: deploy-staging
description: Deploy to staging
category: script
tags: [deploy, staging]
created: "2026-06-18"
updated: "2026-06-18"
status: working
times_used: 3
related: [docker-setup]
---
# Deploy script
Some body text.
"#;
        let entry = MemoryFormat::parse(content).unwrap();
        assert_eq!(entry.name, "deploy-staging");
        assert_eq!(entry.category, MemoryCategory::Script);
        assert_eq!(entry.tags, vec!["deploy", "staging"]);
        assert_eq!(entry.times_used, 3);
        assert_eq!(entry.status, MemoryStatus::Working);
        assert!(entry.body.contains("Some body text"));
    }

    #[test]
    fn parse_missing_frontmatter_returns_none() {
        assert!(MemoryFormat::parse("no frontmatter here").is_none());
    }

    #[test]
    fn serialize_roundtrip() {
        let content = r#"---
name: test
description: Test memory
category: fact
tags: []
created: "2026-06-18"
updated: "2026-06-18"
status: working
times_used: 0
related: []
---
Body text.
"#;
        let entry = MemoryFormat::parse(content).unwrap();
        let serialized = MemoryFormat::serialize(&entry);
        let reparse = MemoryFormat::parse(&serialized).unwrap();
        assert_eq!(entry.name, reparse.name);
        assert_eq!(entry.body, reparse.body);
        assert_eq!(entry.category, reparse.category);
    }

    #[test]
    fn default_status_is_working() {
        assert_eq!(MemoryStatus::default(), MemoryStatus::Working);
    }
}
