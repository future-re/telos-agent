//! Plugin system — marketplace-based extensibility for the agent runtime.
//!
//! A plugin is a directory containing a `plugin.json` manifest that declares
//! which components it provides: tools, hooks, skills, MCP servers, agents,
//! prompt sections, and output styles.
//!
//! Plugins are installed from marketplaces — curated collections fetched from
//! GitHub, git URLs, npm, pip, or local directories.

pub mod errors;
pub mod manifest;
pub mod registry;

use serde::{Deserialize, Serialize};
use std::fmt;

pub use errors::{DependencyReason, PluginError};
pub use manifest::{
    ConfigOptionType, DependencyRef, HookDef, HookMatcher, HooksConfig, LspServerEntry,
    LspServersConfig, MarketplaceEntry, McpServerEntry, McpServersConfig, PartialPluginManifest,
    PluginAuthor, PluginManifest, PluginSource, UserConfigOption,
};
pub use registry::{LoadedPlugin, PluginEntry, PluginRegistry, PluginStatus};

/// Universal plugin identifier: `name@marketplace`.
///
/// Both parts use kebab-case alphanumeric with dots, hyphens, and underscores.
///
/// # Examples
/// - `code-formatter@telos-official`
/// - `my-plugin@builtin`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginId {
    pub name: String,
    pub marketplace: String,
}

/// Sentinel marketplace name for built-in plugins that ship with the binary.
pub const BUILTIN_MARKETPLACE: &str = "builtin";

impl PluginId {
    /// Parse a "name@marketplace" string into a PluginId.
    ///
    /// Returns `None` if the string doesn't contain exactly one `@`.
    pub fn parse(raw: &str) -> Option<Self> {
        let (name, marketplace) = raw.split_once('@')?;
        if name.is_empty() || marketplace.is_empty() {
            return None;
        }
        // Reject multiple @ signs
        if marketplace.contains('@') {
            return None;
        }
        Some(Self { name: name.to_string(), marketplace: marketplace.to_string() })
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.marketplace)
    }
}

impl Serialize for PluginId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PluginId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        PluginId::parse(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid PluginId '{s}': expected 'name@marketplace' format"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_plugin_id() {
        let id = PluginId::parse("foo@bar").unwrap();
        assert_eq!(id.name, "foo");
        assert_eq!(id.marketplace, "bar");
    }

    #[test]
    fn parse_with_dots_and_hyphens() {
        let id = PluginId::parse("code-formatter@telos-official").unwrap();
        assert_eq!(id.name, "code-formatter");
        assert_eq!(id.marketplace, "telos-official");
    }

    #[test]
    fn parse_missing_at_returns_none() {
        assert!(PluginId::parse("foobar").is_none());
    }

    #[test]
    fn parse_empty_name_returns_none() {
        assert!(PluginId::parse("@bar").is_none());
    }

    #[test]
    fn parse_empty_marketplace_returns_none() {
        assert!(PluginId::parse("foo@").is_none());
    }

    #[test]
    fn parse_multiple_at_returns_none() {
        assert!(PluginId::parse("foo@bar@baz").is_none());
    }

    #[test]
    fn display_roundtrips() {
        let id = PluginId { name: "foo".into(), marketplace: "bar".into() };
        assert_eq!(id.to_string(), "foo@bar");
    }

    #[test]
    fn serde_roundtrip() {
        let id = PluginId { name: "foo".into(), marketplace: "bar".into() };
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""foo@bar""#);
        let parsed: PluginId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn serde_invalid_rejected() {
        let result: Result<PluginId, _> = serde_json::from_str(r#""no-at-sign""#);
        assert!(result.is_err());
    }
}
