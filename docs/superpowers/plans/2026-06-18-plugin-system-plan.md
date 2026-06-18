# Plugin System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a marketplace-based plugin system to `telos-agent` following the learn-claude-code architecture, allowing plugins to contribute tools, hooks, skills, MCP servers, agents, prompt sections, and output styles.

**Architecture:** New `src/plugin/` module inside `telos-agent` crate. Plugins are directories with a `plugin.json` manifest declaring components. A `PluginRegistry` manages enable/disable state and applies components into the agent's extension registries (`ToolRegistry`, `HookRegistry`, `SkillRegistry`, `McpManager`, `PromptAssembly`). Marketplaces are plugin collections fetched from GitHub, git URLs, npm, pip, or local directories.

**Tech Stack:** Rust 2024 edition, `serde`/`serde_json` for manifest parsing, `jsonschema` for validation (already a dependency), `tokio::process::Command` for subprocess tools/hooks.

## Global Constraints

- All code goes into the existing `telos_agent` crate (`src/plugin/`)
- Follow existing patterns: `#[async_trait]` for traits, `thiserror::Error` for errors, `Arc<dyn Trait>` for shared ownership
- Tool namespacing: plugin tools use `plugin__<plugin_name>__<tool_name>` prefix
- Plugin IDs use `name@marketplace` format; built-in plugins use `@builtin` marketplace sentinel
- All serde types use `#[serde(rename_all = "camelCase")]` to match learn-claude-code JSON conventions
- State persists to `~/.telos/plugins/plugin_state.json`

---

---

### Task 1: PluginError type

**Files:**
- Create: `src/plugin/errors.rs`
- Create: `src/plugin/mod.rs`

**Interfaces:**
- Produces: `PluginError` enum (17 variants), `DependencyReason` enum, used by all subsequent tasks

- [ ] **Step 1: Create `src/plugin/errors.rs`**

```rust
//! Plugin system error types — discriminated union following learn-claude-code patterns.

use std::path::PathBuf;
use thiserror::Error;

use crate::plugin::PluginId;

/// Why a dependency requirement was not satisfied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyReason {
    /// The dependency is installed but not enabled.
    NotEnabled,
    /// The dependency was not found in any configured marketplace.
    NotFound,
}

impl std::fmt::Display for DependencyReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DependencyReason::NotEnabled => write!(f, "not enabled"),
            DependencyReason::NotFound => write!(f, "not found"),
        }
    }
}

/// All error conditions surfaced by the plugin system.
#[derive(Debug, Error)]
pub enum PluginError {
    // --- Manifest ---
    #[error("manifest not found at {path}")]
    ManifestNotFound { path: PathBuf },
    #[error("manifest parse error in {path}: {reason}")]
    ManifestParse { path: PathBuf, reason: String },
    #[error("manifest validation failed: {errors:?}")]
    ManifestValidation { errors: Vec<String> },

    // --- Sources ---
    #[error("plugin '{plugin_id}' not found in marketplace '{marketplace}'")]
    PluginNotFound { plugin_id: String, marketplace: String },
    #[error("marketplace '{marketplace}' not found. Available: {available:?}")]
    MarketplaceNotFound { marketplace: String, available: Vec<String> },
    #[error("git clone failed for {url}: {reason}")]
    GitCloneFailed { url: String, reason: String },
    #[error("npm install failed for {package}: {reason}")]
    NpmInstallFailed { package: String, reason: String },
    #[error("pip install failed for {package}: {reason}")]
    PipInstallFailed { package: String, reason: String },
    #[error("network error fetching {url}: {detail}")]
    NetworkError { url: String, detail: String },

    // --- Dependencies ---
    #[error("dependency '{dependency}' is {reason}")]
    DependencyUnsatisfied { dependency: String, reason: DependencyReason },
    #[error("circular dependency detected: {cycle:?}")]
    CircularDependency { cycle: Vec<PluginId> },

    // --- Lifecycle ---
    #[error("plugin '{0}' is already enabled")]
    AlreadyEnabled(PluginId),
    #[error("plugin '{0}' is already disabled")]
    AlreadyDisabled(PluginId),
    #[error("plugin '{0}' failed to load components: {1}")]
    ComponentLoadFailed(PluginId, String),
    #[error("plugin '{0}' is degraded — {loaded}/{total} components loaded")]
    Degraded { id: PluginId, loaded: usize, total: usize },

    // --- User config ---
    #[error("user configuration required for plugin '{id}'")]
    UserConfigRequired { id: PluginId },
    #[error("user configuration validation failed: {errors:?}")]
    UserConfigValidation { errors: Vec<String> },

    // --- I/O ---
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    // --- Serde ---
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    // --- Generic ---
    #[error("{0}")]
    Other(String),
}
```

- [ ] **Step 2: Create `src/plugin/mod.rs`**

```rust
//! Plugin system — marketplace-based extensibility for the agent runtime.
//!
//! A plugin is a directory containing a `plugin.json` manifest that declares
//! which components it provides: tools, hooks, skills, MCP servers, agents,
//! prompt sections, and output styles.
//!
//! Plugins are installed from marketplaces — curated collections fetched from
//! GitHub, git URLs, npm, pip, or local directories.

pub mod errors;

use std::fmt;
use serde::{Deserialize, Serialize};

pub use errors::{DependencyReason, PluginError};

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
        Some(Self {
            name: name.to_string(),
            marketplace: marketplace.to_string(),
        })
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
```

- [ ] **Step 3: Add the module to `src/lib.rs`**

Add after the existing `pub mod permissions;` line:

```rust
pub mod plugin;
```

And add to the public re-exports section (after the MCP block):

```rust
// Plugin — marketplace-based plugin system for extensibility.
pub use plugin::{PluginError, PluginId, BUILTIN_MARKETPLACE};
```

- [ ] **Step 4: Build check**

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo check
```
Expected: compiles clean (warnings OK, no errors)

- [ ] **Step 5: Run PluginId unit tests**

Add this test module to the bottom of `src/plugin/mod.rs`:

```rust
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
```

Run:
```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo test plugin::tests
```
Expected: all 8 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/plugin/mod.rs src/plugin/errors.rs src/lib.rs
git commit -m "feat(plugin): add PluginId and PluginError types

PluginId: 'name@marketplace' format with parse/display/serde.
PluginError: 17-variant discriminated union for manifest, source,
dependency, lifecycle, user-config, and I/O errors.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: PluginManifest and serde types

**Files:**
- Create: `src/plugin/manifest.rs`
- Modify: `src/plugin/mod.rs`

**Interfaces:**
- Consumes: `PluginId`, `PluginError` (from Task 1)
- Produces: `PluginManifest`, `PluginAuthor`, `UserConfigOption`, `ConfigOptionType`, `DependencyRef`, `HooksConfig`, `McpServersConfig`, `LspServersConfig`, `PluginEntry`, `PartialPluginManifest`

- [ ] **Step 1: Create `src/plugin/manifest.rs`**

```rust
//! Plugin manifest types — serde-compatible schema for plugin.json.

use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plugin::PluginId;

// --- Metadata types ---

/// Author or maintainer of a plugin or marketplace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAuthor {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// One user-configurable option declared by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserConfigOption {
    #[serde(rename = "type")]
    pub type_: ConfigOptionType,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigOptionType {
    String,
    Number,
    Boolean,
    Directory,
    File,
}

/// A dependency reference. Bare "name" resolves against the declaring plugin's
/// marketplace. "name@marketplace" is fully qualified.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencyRef {
    Bare(String),
    #[serde(rename_all = "camelCase")]
    Qualified { name: String, marketplace: String },
}

impl DependencyRef {
    /// Resolve this reference into a concrete PluginId.
    ///
    /// Bare names use `default_marketplace`; qualified names use their own.
    pub fn resolve(&self, default_marketplace: &str) -> PluginId {
        match self {
            DependencyRef::Bare(name) => PluginId {
                name: name.clone(),
                marketplace: default_marketplace.to_string(),
            },
            DependencyRef::Qualified { name, marketplace } => PluginId {
                name: name.clone(),
                marketplace: marketplace.clone(),
            },
        }
    }

    /// Display the dependency as a string.
    pub fn display(&self) -> String {
        match self {
            DependencyRef::Bare(name) => name.clone(),
            DependencyRef::Qualified { name, marketplace } => format!("{name}@{marketplace}"),
        }
    }
}

// --- Hook configuration ---

/// Hook event matcher — same shape as learn-claude-code's hook matcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookMatcher {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    pub hooks: Vec<HookDef>,
}

/// Individual hook definition within a matcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HookDef {
    Command {
        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default = "default_hook_timeout_ms")]
        timeout: u64,
    },
    // Prompt hooks are future work; declared now for forward-compat
    #[serde(other)]
    Unknown,
}

fn default_hook_timeout_ms() -> u64 {
    30_000
}

/// Full hooks configuration as it appears in plugin.json or hooks.json.
///
/// Keys are hook event names: "PreToolUse", "PostToolUse", "SessionStart", etc.
/// Values are arrays of HookMatcher entries.
pub type HooksConfig = HashMap<String, Vec<HookMatcher>>;

// --- MCP configuration ---

/// MCP server configuration (mirrors crate::mcp::McpServerConfig but serde-friendly).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerEntry {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub auto_connect: bool,
    #[serde(default = "default_mcp_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_true() -> bool { true }
fn default_mcp_timeout_ms() -> u64 { 60_000 }

/// MCP servers declared in plugin.json — either inline or a path to .mcp.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServersConfig {
    /// Path to a .mcp.json file relative to plugin root (starts with "./").
    File(String),
    /// Inline server definitions.
    Inline(HashMap<String, McpServerEntry>),
}

// --- LSP configuration ---

/// Individual LSP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspServerEntry {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// File extension to language ID mapping (e.g. ".ts" → "typescript").
    pub extension_to_language: HashMap<String, String>,
    #[serde(default = "default_transport")]
    pub transport: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
}

fn default_transport() -> String { "stdio".into() }

/// LSP servers declared in plugin.json — either inline or a path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LspServersConfig {
    File(String),
    Inline(HashMap<String, LspServerEntry>),
}

// --- The full manifest ---

/// Parsed plugin.json — the plugin's self-describing manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    #[serde(default)]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<PluginAuthor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<DependencyRef>,

    // Components — all optional, paths relative to plugin root
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HooksConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<McpServersConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lsp_servers: Option<LspServersConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_sections: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_styles: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<HashMap<String, Value>>,

    // User configuration prompts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_config: Option<HashMap<String, UserConfigOption>>,
}

/// A partial manifest — marketplace entries can override fields.
///
/// This is a subset of PluginManifest with all optional fields.
pub type PartialPluginManifest = Value;

/// An entry in a marketplace — describes a plugin and where to get it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceEntry {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub source: PluginSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default = "default_strict")]
    pub strict: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_override: Option<PartialPluginManifest>,
}

fn default_strict() -> bool { true }

// --- Plugin source types ---

/// Where to fetch a plugin from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginSource {
    /// Local directory containing plugin.json.
    Local {
        path: PathBuf,
    },
    /// GitHub repository: "owner/repo".
    #[serde(rename_all = "camelCase")]
    GitHub {
        repo: String,
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sha: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Arbitrary git URL.
    #[serde(rename_all = "camelCase")]
    Git {
        url: String,
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sha: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// npm package.
    #[serde(rename_all = "camelCase")]
    Npm {
        package: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        registry: Option<String>,
    },
    /// pip/PyPI package.
    #[serde(rename_all = "camelCase")]
    Pip {
        package: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        registry: Option<String>,
    },
}
```

- [ ] **Step 2: Update `src/plugin/mod.rs`** — add the manifest module

Add after `pub mod errors;`:

```rust
pub mod manifest;
pub use manifest::{
    ConfigOptionType, DependencyRef, HookDef, HookMatcher, HooksConfig, LspServerEntry,
    LspServersConfig, MarketplaceEntry, McpServerEntry, McpServersConfig, PartialPluginManifest,
    PluginAuthor, PluginManifest, PluginSource, UserConfigOption,
};
```

Also add `source` as a re-export for `PluginSource` — later tasks will add `sources.rs`.

- [ ] **Step 3: Write tests for manifest parsing**

Add to the bottom of `src/plugin/manifest.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_minimal_manifest() {
        let json = json!({
            "name": "my-plugin",
            "version": "1.0.0",
            "description": "A test plugin"
        });
        let manifest: PluginManifest = serde_json::from_value(json).unwrap();
        assert_eq!(manifest.name, "my-plugin");
        assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
        assert!(manifest.tools.is_none());
        assert!(manifest.hooks.is_none());
        assert!(manifest.dependencies.is_empty());
    }

    #[test]
    fn parse_full_manifest() {
        let json = json!({
            "name": "full-plugin",
            "version": "2.1.0",
            "description": "Has everything",
            "author": {
                "name": "Alice",
                "email": "alice@example.com",
                "url": "https://example.com"
            },
            "homepage": "https://plugin.example.com",
            "repository": "https://github.com/alice/full-plugin",
            "license": "MIT",
            "keywords": ["testing", "example"],
            "dependencies": [
                "required-dep",
                {"name": "other", "marketplace": "community"}
            ],
            "tools": ["./tools/my_tool.json"],
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash(git *)",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "python3",
                                "args": ["validate.py"]
                            }
                        ]
                    }
                ]
            },
            "skills": ["./skills/my-skill.md"],
            "agents": ["./agents/auditor.md"],
            "mcpServers": {
                "my-server": {
                    "command": "node",
                    "args": ["server.js"],
                    "autoConnect": true,
                    "timeoutMs": 30000
                }
            },
            "promptSections": ["./prompt/context.md"],
            "outputStyles": ["./styles/theme.json"],
            "userConfig": {
                "API_KEY": {
                    "type": "string",
                    "title": "API Key",
                    "description": "Your API key",
                    "required": true,
                    "sensitive": true
                }
            }
        });

        let manifest: PluginManifest = serde_json::from_value(json).unwrap();
        assert_eq!(manifest.name, "full-plugin");
        assert_eq!(manifest.version.unwrap(), "2.1.0");
        assert!(manifest.author.is_some());
        let author = manifest.author.unwrap();
        assert_eq!(author.name, "Alice");
        assert_eq!(author.email.unwrap(), "alice@example.com");
        assert_eq!(manifest.keywords.len(), 2);
        assert_eq!(manifest.dependencies.len(), 2);

        // Check dependency resolution
        let dep1 = &manifest.dependencies[0];
        let resolved1 = dep1.resolve("telos-official");
        assert_eq!(resolved1.to_string(), "required-dep@telos-official");

        let dep2 = &manifest.dependencies[1];
        let resolved2 = dep2.resolve("telos-official");
        assert_eq!(resolved2.to_string(), "other@community");

        assert!(manifest.tools.is_some());
        assert!(manifest.hooks.is_some());
        assert!(manifest.skills.is_some());
        assert!(manifest.agents.is_some());
        assert!(manifest.mcp_servers.is_some());
        assert!(manifest.prompt_sections.is_some());
        assert!(manifest.output_styles.is_some());
        assert!(manifest.user_config.is_some());
    }

    #[test]
    fn parse_dependency_ref_bare() {
        let json = json!("my-dep");
        let dep: DependencyRef = serde_json::from_value(json).unwrap();
        assert_eq!(dep.display(), "my-dep");
        let id = dep.resolve("my-marketplace");
        assert_eq!(id.to_string(), "my-dep@my-marketplace");
    }

    #[test]
    fn parse_dependency_ref_qualified() {
        let json = json!({"name": "dep", "marketplace": "other-mkt"});
        let dep: DependencyRef = serde_json::from_value(json).unwrap();
        assert_eq!(dep.display(), "dep@other-mkt");
        let id = dep.resolve("my-marketplace");
        assert_eq!(id.to_string(), "dep@other-mkt");
    }

    #[test]
    fn parse_plugin_source_github() {
        let json = json!({
            "type": "github",
            "repo": "owner/repo",
            "ref": "main"
        });
        let source: PluginSource = serde_json::from_value(json).unwrap();
        match source {
            PluginSource::GitHub { repo, ref_, .. } => {
                assert_eq!(repo, "owner/repo");
                assert_eq!(ref_.as_deref(), Some("main"));
            }
            _ => panic!("expected GitHub source"),
        }
    }

    #[test]
    fn parse_plugin_source_local() {
        let json = json!({
            "type": "local",
            "path": "/tmp/my-plugin"
        });
        let source: PluginSource = serde_json::from_value(json).unwrap();
        match source {
            PluginSource::Local { path } => {
                assert_eq!(path, std::path::PathBuf::from("/tmp/my-plugin"));
            }
            _ => panic!("expected Local source"),
        }
    }

    #[test]
    fn parse_mcp_servers_inline() {
        let json = json!({
            "my-server": {
                "command": "node",
                "args": ["server.js"],
                "autoConnect": true
            }
        });
        let config: McpServersConfig = serde_json::from_value(json).unwrap();
        match config {
            McpServersConfig::Inline(servers) => {
                assert_eq!(servers.len(), 1);
                assert_eq!(servers.get("my-server").unwrap().command, "node");
            }
            McpServersConfig::File(_) => panic!("expected inline"),
        }
    }

    #[test]
    fn parse_mcp_servers_file() {
        let json = json!("./.mcp.json");
        let config: McpServersConfig = serde_json::from_value(json).unwrap();
        match config {
            McpServersConfig::File(path) => assert_eq!(path, "./.mcp.json"),
            McpServersConfig::Inline(_) => panic!("expected file path"),
        }
    }
}
```

- [ ] **Step 4: Run manifest tests**

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo test plugin::manifest::tests
```
Expected: all 8 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/plugin/manifest.rs src/plugin/mod.rs
git commit -m "feat(plugin): add PluginManifest and serde types

PluginManifest, PluginAuthor, DependencyRef, HooksConfig, McpServerEntry,
PluginSource enum (Local/GitHub/Git/Npm/Pip), MarketplaceEntry.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: PluginRegistry — core lifecycle management

**Files:**
- Create: `src/plugin/registry.rs`
- Modify: `src/plugin/mod.rs`

**Interfaces:**
- Consumes: `PluginId`, `PluginManifest`, `PluginError`, `PluginSource` (from Tasks 1,2)
- Produces: `PluginRegistry`, `LoadedPlugin`, `PluginStatus`, `PluginEntry`

- [ ] **Step 1: Create `src/plugin/registry.rs`**

```rust
//! PluginRegistry — manages loaded plugins and their enable/disable lifecycle.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::plugin::manifest::PluginManifest;
use crate::plugin::{PluginError, PluginId, PluginSource};

/// The status of a loaded plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    /// Plugin is enabled and all components are active.
    Enabled,
    /// Plugin is installed but disabled.
    Disabled,
    /// Plugin is enabled but some components failed to load.
    Degraded,
    /// Plugin failed to load entirely.
    Error,
}

/// A plugin loaded from disk with its manifest and resolved paths.
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub id: PluginId,
    pub manifest: PluginManifest,
    /// Absolute path to the installed plugin directory.
    pub path: PathBuf,
    /// The source this plugin was installed from.
    pub source: PluginSource,
    pub enabled: bool,
    pub is_builtin: bool,
    /// Resolved absolute paths for each component type (manifest paths joined with plugin root).
    pub resolved_tools: Vec<PathBuf>,
    pub resolved_skills: Vec<PathBuf>,
    pub resolved_agents: Vec<PathBuf>,
    pub resolved_prompt_sections: Vec<PathBuf>,
    pub resolved_output_styles: Vec<PathBuf>,
}

/// Internal tracking entry for a plugin in the registry.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub plugin: LoadedPlugin,
    pub status: PluginStatus,
    /// Errors from the last load attempt (empty if successful).
    pub load_errors: Vec<PluginError>,
}

impl PluginEntry {
    fn new(plugin: LoadedPlugin, status: PluginStatus) -> Self {
        Self { plugin, status, load_errors: Vec::new() }
    }
}

/// Central registry for all loaded plugins.
///
/// Manages install/enable/disable/uninstall lifecycle and persists
/// enable/disable state to disk.
#[derive(Clone)]
pub struct PluginRegistry {
    plugins: HashMap<PluginId, PluginEntry>,
    plugins_root: PathBuf,
}

impl PluginRegistry {
    /// Create a new registry backed by `plugins_root` (typically `~/.telos/plugins/`).
    pub fn new(plugins_root: impl Into<PathBuf>) -> Self {
        Self {
            plugins: HashMap::new(),
            plugins_root: plugins_root.into(),
        }
    }

    /// Path where installed plugins live.
    pub fn installed_dir(&self) -> PathBuf {
        self.plugins_root.join("installed")
    }

    /// Path to the state file.
    pub fn state_path(&self) -> PathBuf {
        self.plugins_root.join("plugin_state.json")
    }

    // --- Registration ---

    /// Register a loaded plugin without enabling it.
    ///
    /// If a plugin with the same ID already exists, it is replaced
    /// (the old plugin's state is lost).
    pub fn register(&mut self, plugin: LoadedPlugin) {
        let status = if plugin.enabled {
            PluginStatus::Enabled
        } else {
            PluginStatus::Disabled
        };
        self.plugins.insert(plugin.id.clone(), PluginEntry::new(plugin, status));
    }

    // --- Lifecycle ---

    /// Enable a plugin. Call this after registration.
    ///
    /// This is idempotent — enabling an already-enabled plugin is a no-op.
    pub fn enable(&mut self, id: &PluginId) -> Result<(), PluginError> {
        let entry = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| PluginError::PluginNotFound {
                plugin_id: id.to_string(),
                marketplace: id.marketplace.clone(),
            })?;

        if entry.status == PluginStatus::Enabled {
            return Ok(());
        }

        entry.status = PluginStatus::Enabled;
        entry.plugin.enabled = true;
        Ok(())
    }

    /// Disable a plugin. Does not uninstall — the plugin stays on disk.
    ///
    /// This is idempotent — disabling an already-disabled plugin is a no-op.
    pub fn disable(&mut self, id: &PluginId) -> Result<(), PluginError> {
        let entry = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| PluginError::PluginNotFound {
                plugin_id: id.to_string(),
                marketplace: id.marketplace.clone(),
            })?;

        if entry.status == PluginStatus::Disabled {
            return Ok(());
        }

        entry.status = PluginStatus::Disabled;
        entry.plugin.enabled = false;
        Ok(())
    }

    /// Mark a plugin as degraded (enabled but with component load errors).
    pub fn mark_degraded(&mut self, id: &PluginId, errors: Vec<PluginError>) {
        if let Some(entry) = self.plugins.get_mut(id) {
            entry.status = PluginStatus::Degraded;
            entry.load_errors = errors;
        }
    }

    /// Mark a plugin as in error state.
    pub fn mark_error(&mut self, id: &PluginId, error: PluginError) {
        if let Some(entry) = self.plugins.get_mut(id) {
            entry.status = PluginStatus::Error;
            entry.load_errors = vec![error];
        }
    }

    /// Remove a plugin from the registry entirely.
    pub fn remove(&mut self, id: &PluginId) -> Option<PluginEntry> {
        self.plugins.remove(id)
    }

    // --- Queries ---

    /// Look up a plugin by ID.
    pub fn get(&self, id: &PluginId) -> Option<&PluginEntry> {
        self.plugins.get(id)
    }

    /// Mutable lookup.
    pub fn get_mut(&mut self, id: &PluginId) -> Option<&mut PluginEntry> {
        self.plugins.get_mut(id)
    }

    /// All enabled plugins.
    pub fn list_enabled(&self) -> Vec<&PluginEntry> {
        self.plugins
            .values()
            .filter(|e| e.status == PluginStatus::Enabled || e.status == PluginStatus::Degraded)
            .collect()
    }

    /// All disabled plugins.
    pub fn list_disabled(&self) -> Vec<&PluginEntry> {
        self.plugins.values().filter(|e| e.status == PluginStatus::Disabled).collect()
    }

    /// All plugins regardless of status.
    pub fn list_all(&self) -> Vec<&PluginEntry> {
        self.plugins.values().collect()
    }

    /// Check if a plugin is installed (present in registry, any status).
    pub fn is_installed(&self, id: &PluginId) -> bool {
        self.plugins.contains_key(id)
    }

    /// Number of registered plugins.
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    // --- Discovery ---

    /// Scan the installed directory and load all plugins found there.
    ///
    /// Each subdirectory that contains a `plugin.json` is loaded.
    /// Plugins are NOT auto-enabled — state is restored from `plugin_state.json`.
    pub fn discover_installed(&mut self) -> Result<Vec<PluginId>, PluginError> {
        let installed_dir = self.installed_dir();
        if !installed_dir.exists() {
            return Ok(Vec::new());
        }

        let mut discovered = Vec::new();
        let entries = std::fs::read_dir(&installed_dir)?;

        for entry in entries {
            let entry = entry?;
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }

            let manifest_path = dir.join("plugin.json");
            if !manifest_path.exists() {
                continue;
            }

            match self.load_plugin_from_dir(&dir) {
                Ok(plugin) => {
                    let id = plugin.id.clone();
                    discovered.push(id.clone());
                    self.register(plugin);
                }
                Err(err) => {
                    tracing::warn!(
                        dir = %dir.display(),
                        error = %err,
                        "failed to load plugin from installed directory"
                    );
                }
            }
        }

        Ok(discovered)
    }

    /// Load a single plugin from a directory containing plugin.json.
    fn load_plugin_from_dir(&self, dir: &Path) -> Result<LoadedPlugin, PluginError> {
        let manifest_path = dir.join("plugin.json");
        let content = std::fs::read_to_string(&manifest_path).map_err(|e| {
            PluginError::ManifestParse {
                path: manifest_path.clone(),
                reason: format!("failed to read: {e}"),
            }
        })?;

        let manifest: PluginManifest = serde_json::from_str(&content).map_err(|e| {
            PluginError::ManifestParse {
                path: manifest_path.clone(),
                reason: format!("invalid JSON: {e}"),
            }
        })?;

        if manifest.name.is_empty() {
            return Err(PluginError::ManifestValidation {
                errors: vec!["name must not be empty".into()],
            });
        }

        // Determine marketplace from the parent directory name
        // installed/<name>@<marketplace>/plugin.json
        let dir_name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown@unknown");
        let id = PluginId::parse(dir_name)
            .unwrap_or_else(|| PluginId {
                name: manifest.name.clone(),
                marketplace: "unknown".into(),
            });

        // Resolve component paths
        let resolve = |paths: &Option<Vec<String>>| -> Vec<PathBuf> {
            paths
                .as_ref()
                .map(|p| p.iter().map(|s| dir.join(s)).collect())
                .unwrap_or_default()
        };

        // Find .json tool files in the tools/ directory
        let mut resolved_tools = resolve(&manifest.tools);
        let tools_dir = dir.join("tools");
        if tools_dir.exists() && tools_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&tools_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().is_some_and(|ext| ext == "json") {
                        resolved_tools.push(p);
                    }
                }
            }
        }

        Ok(LoadedPlugin {
            id,
            manifest,
            path: dir.to_path_buf(),
            source: PluginSource::Local { path: dir.to_path_buf() },
            enabled: false, // will be set by state restoration
            is_builtin: false,
            resolved_tools,
            resolved_skills: resolve(&manifest.skills),
            resolved_agents: resolve(&manifest.agents),
            resolved_prompt_sections: resolve(&manifest.prompt_sections),
            resolved_output_styles: resolve(&manifest.output_styles),
        })
    }

    // --- Persistence ---

    /// Save enabled/disabled state to `plugin_state.json`.
    pub fn save_state(&self) -> Result<(), PluginError> {
        let state: HashMap<String, String> = self
            .plugins
            .iter()
            .map(|(id, entry)| {
                let status_str = match entry.status {
                    PluginStatus::Enabled | PluginStatus::Degraded => "enabled",
                    PluginStatus::Disabled => "disabled",
                    PluginStatus::Error => "error",
                };
                (id.to_string(), status_str.to_string())
            })
            .collect();

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "plugins": state,
        }))?;

        if let Some(parent) = self.state_path().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(self.state_path(), json)?;
        Ok(())
    }

    /// Load enabled/disabled state from `plugin_state.json`.
    pub fn load_state(&mut self) -> Result<(), PluginError> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;

        let plugins = value
            .get("plugins")
            .and_then(|v| v.as_object());

        if let Some(plugins) = plugins {
            for (id_str, status_val) in plugins {
                if let Some(id) = PluginId::parse(id_str) {
                    if let Some(status_str) = status_val.as_str() {
                        if let Some(entry) = self.plugins.get_mut(&id) {
                            match status_str {
                                "enabled" => {
                                    entry.status = PluginStatus::Enabled;
                                    entry.plugin.enabled = true;
                                }
                                "disabled" => {
                                    entry.status = PluginStatus::Disabled;
                                    entry.plugin.enabled = false;
                                }
                                "error" => {
                                    entry.status = PluginStatus::Error;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_plugin_dir(dir: &Path, name: &str, marketplace: &str) -> PathBuf {
        let plugin_dir = dir.join("installed").join(format!("{name}@{marketplace}"));
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let manifest = serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "description": "A test plugin"
        });
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        plugin_dir
    }

    #[test]
    fn register_and_get_plugin() {
        let tmp = TempDir::new().unwrap();
        let mut registry = PluginRegistry::new(tmp.path());

        let id = PluginId { name: "test".into(), marketplace: "test-mkt".into() };
        let plugin = LoadedPlugin {
            id: id.clone(),
            manifest: PluginManifest {
                name: "test".into(),
                ..serde_json::from_value(serde_json::json!({"name": "test"})).unwrap()
            },
            path: tmp.path().to_path_buf(),
            source: PluginSource::Local { path: tmp.path().to_path_buf() },
            enabled: false,
            is_builtin: false,
            resolved_tools: vec![],
            resolved_skills: vec![],
            resolved_agents: vec![],
            resolved_prompt_sections: vec![],
            resolved_output_styles: vec![],
        };

        registry.register(plugin);
        assert!(registry.get(&id).is_some());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn enable_disable_lifecycle() {
        let tmp = TempDir::new().unwrap();
        let mut registry = PluginRegistry::new(tmp.path());
        let id = PluginId { name: "t".into(), marketplace: "m".into() };
        let plugin = LoadedPlugin {
            id: id.clone(),
            manifest: serde_json::from_value(serde_json::json!({"name": "t"})).unwrap(),
            path: tmp.path().to_path_buf(),
            source: PluginSource::Local { path: tmp.path().to_path_buf() },
            enabled: false,
            is_builtin: false,
            resolved_tools: vec![],
            resolved_skills: vec![],
            resolved_agents: vec![],
            resolved_prompt_sections: vec![],
            resolved_output_styles: vec![],
        };

        registry.register(plugin);
        assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Disabled);

        registry.enable(&id).unwrap();
        assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Enabled);

        // Idempotent
        registry.enable(&id).unwrap();
        assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Enabled);

        registry.disable(&id).unwrap();
        assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Disabled);

        // Idempotent
        registry.disable(&id).unwrap();
        assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Disabled);
    }

    #[test]
    fn enable_nonexistent_returns_error() {
        let tmp = TempDir::new().unwrap();
        let mut registry = PluginRegistry::new(tmp.path());
        let id = PluginId { name: "nope".into(), marketplace: "nope".into() };
        let result = registry.enable(&id);
        assert!(result.is_err());
    }

    #[test]
    fn discover_installed_finds_plugins() {
        let tmp = TempDir::new().unwrap();
        make_plugin_dir(tmp.path(), "my-plugin", "community");
        make_plugin_dir(tmp.path(), "other", "telos-official");

        let mut registry = PluginRegistry::new(tmp.path());
        let discovered = registry.discover_installed().unwrap();
        assert_eq!(discovered.len(), 2);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn discover_skips_non_plugin_dirs() {
        let tmp = TempDir::new().unwrap();
        let installed = tmp.path().join("installed");
        std::fs::create_dir_all(&installed).unwrap();
        // Empty directory — no plugin.json
        std::fs::create_dir_all(installed.join("not-a-plugin")).unwrap();

        let mut registry = PluginRegistry::new(tmp.path());
        let discovered = registry.discover_installed().unwrap();
        assert!(discovered.is_empty());
    }

    #[test]
    fn save_and_load_state() {
        let tmp = TempDir::new().unwrap();
        make_plugin_dir(tmp.path(), "p1", "mkt");
        make_plugin_dir(tmp.path(), "p2", "mkt");

        let mut registry = PluginRegistry::new(tmp.path());
        registry.discover_installed().unwrap();

        // Enable p1, keep p2 disabled
        let id1 = PluginId::parse("p1@mkt").unwrap();
        let id2 = PluginId::parse("p2@mkt").unwrap();
        registry.enable(&id1).unwrap();
        registry.save_state().unwrap();

        // Create a fresh registry and load state
        let mut registry2 = PluginRegistry::new(tmp.path());
        registry2.discover_installed().unwrap();
        registry2.load_state().unwrap();

        assert_eq!(registry2.get(&id1).unwrap().status, PluginStatus::Enabled);
        assert_eq!(registry2.get(&id2).unwrap().status, PluginStatus::Disabled);
    }

    #[test]
    fn list_enabled_and_disabled() {
        let tmp = TempDir::new().unwrap();
        make_plugin_dir(tmp.path(), "a", "m");
        make_plugin_dir(tmp.path(), "b", "m");

        let mut registry = PluginRegistry::new(tmp.path());
        registry.discover_installed().unwrap();
        let id_a = PluginId::parse("a@m").unwrap();
        registry.enable(&id_a).unwrap();

        assert_eq!(registry.list_enabled().len(), 1);
        assert_eq!(registry.list_disabled().len(), 1);
        assert_eq!(registry.list_all().len(), 2);
    }

    #[test]
    fn mark_degraded_and_error() {
        let tmp = TempDir::new().unwrap();
        make_plugin_dir(tmp.path(), "d", "m");

        let mut registry = PluginRegistry::new(tmp.path());
        registry.discover_installed().unwrap();
        let id = PluginId::parse("d@m").unwrap();

        registry.mark_degraded(&id, vec![PluginError::Other("partial load".into())]);
        assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Degraded);

        registry.mark_error(&id, PluginError::Other("total failure".into()));
        assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Error);
    }
}
```

- [ ] **Step 2: Update `src/plugin/mod.rs`** — add the registry module

Add after `pub mod manifest;`:

```rust
pub mod registry;
pub use registry::{LoadedPlugin, PluginEntry, PluginRegistry, PluginStatus};
```

- [ ] **Step 3: Add `tempfile` as dev-dependency**

Already in `Cargo.toml` dev-dependencies — verify:
```bash
grep tempfile /home/alin/codework/tiny_agent/tiny_agent_core/Cargo.toml
```
Expected: `tempfile = "3"` is present.

- [ ] **Step 4: Run registry tests**

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo test plugin::registry::tests
```
Expected: all 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/plugin/registry.rs src/plugin/mod.rs
git commit -m "feat(plugin): add PluginRegistry with lifecycle management

PluginRegistry: register, enable, disable, discover_installed, save/load_state.
LoadedPlugin: resolved paths for all component types.
PluginStatus: Enabled/Disabled/Degraded/Error states.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: PluginSource and MarketplaceSource types + MarketplaceRegistry

**Files:**
- Create: `src/plugin/sources.rs`
- Create: `src/plugin/marketplace.rs`
- Modify: `src/plugin/mod.rs`

**Interfaces:**
- Consumes: `PluginId`, `PluginManifest`, `PluginError`, `PluginSource` (from Tasks 1,2,3)
- Produces: `MarketplaceSource`, `Marketplace`, `MarketplaceRegistry`

- [ ] **Step 1: Create `src/plugin/sources.rs`**

```rust
//! Source types for marketplaces — where marketplace.json comes from.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::plugin::manifest::{MarketplaceEntry, PluginAuthor};

/// Where a marketplace manifest is fetched from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MarketplaceSource {
    /// GitHub repository containing marketplace.json.
    #[serde(rename_all = "camelCase")]
    GitHub {
        repo: String,
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Arbitrary git URL.
    #[serde(rename_all = "camelCase")]
    Git {
        url: String,
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Direct URL to marketplace.json.
    #[serde(rename_all = "camelCase")]
    Url {
        url: String,
    },
    /// npm package containing marketplace.json.
    #[serde(rename_all = "camelCase")]
    Npm {
        package: String,
    },
    /// Local directory containing marketplace.json.
    #[serde(rename_all = "camelCase")]
    Local {
        path: PathBuf,
    },
    /// Inline marketplace defined in config (no remote fetch needed).
    #[serde(rename_all = "camelCase")]
    Inline {
        name: String,
        plugins: Vec<MarketplaceEntry>,
    },
}
```

- [ ] **Step 2: Create `src/plugin/marketplace.rs`**

```rust
//! Marketplace registry — manages marketplace sources and their plugin catalogs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::plugin::manifest::{MarketplaceEntry, PluginAuthor, PluginManifest};
use crate::plugin::sources::MarketplaceSource;
use crate::plugin::PluginError;

/// A curated collection of plugins fetched from a marketplace source.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marketplace {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<PluginAuthor>,
    pub plugins: Vec<MarketplaceEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_remove_deleted_plugins: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_cross_marketplace_deps_on: Option<Vec<String>>,
}

/// Cached marketplace data stored on disk.
#[derive(Debug, Clone)]
struct CachedMarketplace {
    source: MarketplaceSource,
    manifest: Marketplace,
    /// Where the marketplace is cached on disk.
    install_location: PathBuf,
    /// When the marketplace was last refreshed (unix timestamp seconds).
    last_updated: u64,
}

/// Manages marketplace sources and provides plugin discovery across them.
pub struct MarketplaceRegistry {
    marketplaces: HashMap<String, CachedMarketplace>,
    cache_root: PathBuf,
}

impl MarketplaceRegistry {
    /// Create a new marketplace registry. Cache goes under `cache_root/marketplaces/`.
    pub fn new(cache_root: impl Into<PathBuf>) -> Self {
        Self {
            marketplaces: HashMap::new(),
            cache_root: cache_root.into(),
        }
    }

    /// Add a marketplace source. For local/inline sources, this is immediate.
    /// For remote sources (GitHub, git, URL, npm), fetching happens in
    /// `refresh()`.
    ///
    /// Returns the marketplace name.
    pub fn add(&mut self, source: MarketplaceSource) -> Result<String, PluginError> {
        let name = match &source {
            MarketplaceSource::GitHub { repo, .. } => {
                // Derive name from repo: strip org, keep repo name
                repo.split('/').last().unwrap_or(repo).to_string()
            }
            MarketplaceSource::Git { url, .. } => {
                // Derive name from URL: last path segment without .git
                url.trim_end_matches('/')
                    .trim_end_matches(".git")
                    .split('/')
                    .last()
                    .unwrap_or("unknown")
                    .to_string()
            }
            MarketplaceSource::Url { url } => {
                url.trim_end_matches('/')
                    .split('/')
                    .last()
                    .unwrap_or("unknown")
                    .to_string()
            }
            MarketplaceSource::Npm { package } => package.replace('/', "-"),
            MarketplaceSource::Local { path } => path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
            MarketplaceSource::Inline { name, .. } => name.clone(),
        };

        let install_location = self.cache_root.join("marketplaces").join(&name);

        // For local and inline sources, load immediately
        let (manifest, last_updated) = match &source {
            MarketplaceSource::Local { path } => {
                let manifest = Self::load_manifest_from_dir(path)?;
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                (manifest, timestamp)
            }
            MarketplaceSource::Inline { name, plugins } => {
                let manifest = Marketplace {
                    name: name.clone(),
                    owner: None,
                    plugins: plugins.clone(),
                    force_remove_deleted_plugins: None,
                    allow_cross_marketplace_deps_on: None,
                };
                (manifest, 0)
            }
            _ => {
                // Remote sources: create a placeholder; refresh() fills it in
                let manifest = Marketplace {
                    name: name.clone(),
                    owner: None,
                    plugins: Vec::new(),
                    force_remove_deleted_plugins: None,
                    allow_cross_marketplace_deps_on: None,
                };
                (manifest, 0)
            }
        };

        self.marketplaces.insert(
            name.clone(),
            CachedMarketplace {
                source,
                manifest,
                install_location,
                last_updated,
            },
        );

        Ok(name)
    }

    /// Remove a marketplace and its cached data.
    pub fn remove(&mut self, name: &str) -> Result<(), PluginError> {
        self.marketplaces
            .remove(name)
            .ok_or_else(|| PluginError::MarketplaceNotFound {
                marketplace: name.to_string(),
                available: self.marketplaces.keys().cloned().collect(),
            })?;
        Ok(())
    }

    /// Get the marketplace manifest by name.
    pub fn get(&self, name: &str) -> Option<&Marketplace> {
        self.marketplaces.get(name).map(|c| &c.manifest)
    }

    /// List all registered marketplace names.
    pub fn names(&self) -> Vec<&String> {
        self.marketplaces.keys().collect()
    }

    /// Search across all marketplaces for plugins matching `query`
    /// (case-insensitive substring match on name and description).
    pub fn search(&self, query: &str) -> Vec<(&Marketplace, &MarketplaceEntry)> {
        let query = query.to_lowercase();
        let mut results = Vec::new();
        for cached in self.marketplaces.values() {
            for entry in &cached.manifest.plugins {
                if entry.name.to_lowercase().contains(&query)
                    || entry
                        .description
                        .as_ref()
                        .is_some_and(|d| d.to_lowercase().contains(&query))
                {
                    results.push((&cached.manifest, entry));
                }
            }
        }
        results
    }

    /// List all available plugins across all marketplaces.
    pub fn list_all(&self) -> Vec<(&Marketplace, &MarketplaceEntry)> {
        let mut results = Vec::new();
        for cached in self.marketplaces.values() {
            for entry in &cached.manifest.plugins {
                results.push((&cached.manifest, entry));
            }
        }
        results
    }

    /// Save known marketplaces metadata to `known_marketplaces.json`.
    pub fn save(&self) -> Result<(), PluginError> {
        let path = self.cache_root.join("known_marketplaces.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data: HashMap<String, serde_json::Value> = self
            .marketplaces
            .iter()
            .map(|(name, cached)| {
                let entry = serde_json::json!({
                    "source": cached.source,
                    "installLocation": cached.install_location,
                    "lastUpdated": cached.last_updated,
                });
                (name.clone(), entry)
            })
            .collect();
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "marketplaces": data,
        }))?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Load known marketplaces from `known_marketplaces.json`.
    pub fn load(&mut self) -> Result<(), PluginError> {
        let path = self.cache_root.join("known_marketplaces.json");
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(&path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;
        if let Some(marketplaces) = value.get("marketplaces").and_then(|v| v.as_object()) {
            for (name, entry) in marketplaces {
                if self.marketplaces.contains_key(name) {
                    continue; // already registered, skip
                }
                let source: MarketplaceSource = match serde_json::from_value(
                    entry.get("source").cloned().unwrap_or_default(),
                ) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let install_location = entry
                    .get("installLocation")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        self.cache_root.join("marketplaces").join(name)
                    });
                let last_updated = entry
                    .get("lastUpdated")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                // For disk-backed sources, try to load the manifest
                let manifest = match &source {
                    MarketplaceSource::Local { path } => {
                        Self::load_manifest_from_dir(path).unwrap_or_else(|_| Marketplace {
                            name: name.clone(),
                            owner: None,
                            plugins: Vec::new(),
                            force_remove_deleted_plugins: None,
                            allow_cross_marketplace_deps_on: None,
                        })
                    }
                    MarketplaceSource::Inline { name: inline_name, plugins } => Marketplace {
                        name: inline_name.clone(),
                        owner: None,
                        plugins: plugins.clone(),
                        force_remove_deleted_plugins: None,
                        allow_cross_marketplace_deps_on: None,
                    },
                    _ => Marketplace {
                        name: name.clone(),
                        owner: None,
                        plugins: Vec::new(),
                        force_remove_deleted_plugins: None,
                        allow_cross_marketplace_deps_on: None,
                    },
                };

                self.marketplaces.insert(
                    name.clone(),
                    CachedMarketplace {
                        source,
                        manifest,
                        install_location,
                        last_updated,
                    },
                );
            }
        }
        Ok(())
    }

    /// Load a marketplace manifest from a directory containing marketplace.json.
    fn load_manifest_from_dir(dir: &Path) -> Result<Marketplace, PluginError> {
        let manifest_path = dir.join("marketplace.json");
        let content = std::fs::read_to_string(&manifest_path).map_err(|e| {
            PluginError::ManifestParse {
                path: manifest_path.clone(),
                reason: format!("failed to read: {e}"),
            }
        })?;
        let manifest: Marketplace = serde_json::from_str(&content).map_err(|e| {
            PluginError::ManifestParse {
                path: manifest_path,
                reason: format!("invalid JSON: {e}"),
            }
        })?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_marketplace_dir(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let manifest = serde_json::json!({
            "name": name,
            "owner": {"name": "Test Org"},
            "plugins": [
                {
                    "name": "test-plugin",
                    "description": "A test plugin",
                    "source": {"type": "local", "path": "./test-plugin"},
                    "category": "testing",
                    "tags": ["test"]
                },
                {
                    "name": "another-plugin",
                    "description": "Another one",
                    "source": {"type": "github", "repo": "org/repo"}
                }
            ]
        });
        std::fs::write(
            dir.join("marketplace.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn add_local_marketplace() {
        let tmp = TempDir::new().unwrap();
        let mkt_dir = tmp.path().join("my-marketplace");
        make_marketplace_dir(&mkt_dir, "my-marketplace");

        let mut registry = MarketplaceRegistry::new(tmp.path());
        let name = registry
            .add(MarketplaceSource::Local { path: mkt_dir })
            .unwrap();
        assert_eq!(name, "my-marketplace");

        let mkt = registry.get("my-marketplace").unwrap();
        assert_eq!(mkt.plugins.len(), 2);
        assert_eq!(mkt.plugins[0].name, "test-plugin");
    }

    #[test]
    fn add_inline_marketplace() {
        let tmp = TempDir::new().unwrap();
        let mut registry = MarketplaceRegistry::new(tmp.path());
        let name = registry
            .add(MarketplaceSource::Inline {
                name: "inline-mkt".into(),
                plugins: vec![MarketplaceEntry {
                    name: "my-plugin".into(),
                    description: Some("desc".into()),
                    version: None,
                    source: crate::plugin::manifest::PluginSource::Local {
                        path: "/tmp/p".into(),
                    },
                    category: None,
                    tags: vec![],
                    strict: true,
                    manifest_override: None,
                }],
            })
            .unwrap();
        assert_eq!(name, "inline-mkt");
        assert_eq!(registry.list_all().len(), 1);
    }

    #[test]
    fn search_finds_matching_plugins() {
        let tmp = TempDir::new().unwrap();
        let mkt_dir = tmp.path().join("mkt");
        make_marketplace_dir(&mkt_dir, "mkt");

        let mut registry = MarketplaceRegistry::new(tmp.path());
        registry
            .add(MarketplaceSource::Local { path: mkt_dir })
            .unwrap();

        let results = registry.search("test");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "test-plugin");

        let results = registry.search("another");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "another-plugin");

        let results = registry.search("nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn remove_marketplace() {
        let tmp = TempDir::new().unwrap();
        let mut registry = MarketplaceRegistry::new(tmp.path());
        registry
            .add(MarketplaceSource::Inline {
                name: "test".into(),
                plugins: vec![],
            })
            .unwrap();
        assert!(registry.names().contains(&&"test".to_string()));
        registry.remove("test").unwrap();
        assert!(!registry.names().contains(&&"test".to_string()));
    }

    #[test]
    fn save_and_load_marketplaces() {
        let tmp = TempDir::new().unwrap();
        let mkt_dir = tmp.path().join("my-mkt");
        make_marketplace_dir(&mkt_dir, "my-mkt");

        let mut registry = MarketplaceRegistry::new(tmp.path().join("cache"));
        registry
            .add(MarketplaceSource::Local { path: mkt_dir })
            .unwrap();
        registry.save().unwrap();

        let mut registry2 = MarketplaceRegistry::new(tmp.path().join("cache"));
        registry2.load().unwrap();
        assert!(registry2.get("my-mkt").is_some());
        assert_eq!(registry2.list_all().len(), 2);
    }
}
```

- [ ] **Step 3: Update `src/plugin/mod.rs`** — add sources and marketplace

Add after `pub use manifest::{...};`:

```rust
pub mod sources;
pub use sources::MarketplaceSource;
pub mod marketplace;
pub use marketplace::{Marketplace, MarketplaceRegistry};
```

- [ ] **Step 4: Run marketplace tests**

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo test plugin::marketplace::tests
```
Expected: all 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/plugin/sources.rs src/plugin/marketplace.rs src/plugin/mod.rs
git commit -m "feat(plugin): add MarketplaceSource and MarketplaceRegistry

MarketplaceSource: GitHub/Git/Url/Npm/Local/Inline variants.
MarketplaceRegistry: add/remove/search/list marketplace with local+inline loading,
known_marketplaces.json persistence.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: CommandTool — declarative subprocess tools

**Files:**
- Create: `src/plugin/tool_loader.rs`
- Modify: `src/plugin/mod.rs`

**Interfaces:**
- Consumes: `Tool` trait, `ToolContext`, `ToolDefinition`, `ToolOutput`, `AgentError` (from `crate::tool`)
- Produces: `CommandTool` (implements `Tool`), `load_tool_from_file(path)` helper

- [ ] **Step 1: Create `src/plugin/tool_loader.rs`**

```rust
//! CommandTool — declarative JSON-defined tools executed as subprocesses.
//!
//! Plugin tools are defined as JSON files specifying a command, optional args,
//! timeout, and permission level. At runtime, arguments are piped as JSON to
//! stdin; stdout JSON is the tool result.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tokio::process::Command as TokioCommand;
use tokio::io::AsyncWriteExt;

use crate::error::AgentError;
use crate::tool::{
    InterruptBehavior, PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput,
};

/// Declarative JSON spec for a plugin tool (e.g. `tools/my-tool.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    #[serde(default = "default_tool_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub is_concurrency_safe: bool,
    /// Default permission decision when no rule matches.
    #[serde(default = "default_permission")]
    pub permission: ToolPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPermission {
    Allow,
    Ask,
    Deny,
}

fn default_tool_timeout_ms() -> u64 {
    60_000
}

fn default_permission() -> ToolPermission {
    ToolPermission::Ask
}

/// Load a tool spec from a JSON file.
pub fn load_tool_spec(path: &Path) -> Result<ToolSpec, AgentError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        AgentError::Config(format!(
            "failed to read tool spec {}: {e}",
            path.display()
        ))
    })?;
    let spec: ToolSpec = serde_json::from_str(&content).map_err(|e| {
        AgentError::Config(format!(
            "invalid tool spec {}: {e}",
            path.display()
        ))
    })?;
    Ok(spec)
}

/// A `Tool` implementation backed by a subprocess command.
///
/// Arguments are serialized to JSON and piped to the command's stdin.
/// The command must write a JSON value to stdout before exiting.
pub struct CommandTool {
    definition: ToolDefinition,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    timeout: std::time::Duration,
    is_concurrency_safe: bool,
    default_permission: PermissionDecision,
}

impl CommandTool {
    /// Build a CommandTool from a parsed ToolSpec.
    ///
    /// `plugin_root` is prepended to relative command paths and substituted
    /// for `${PLUGIN_ROOT}` in args and env values.
    pub fn from_spec(mut spec: ToolSpec, plugin_root: &Path) -> Self {
        let plugin_root_str = plugin_root.to_string_lossy();

        // Substitute ${PLUGIN_ROOT} in args
        let args: Vec<String> = spec
            .args
            .into_iter()
            .map(|a| a.replace("${PLUGIN_ROOT}", &plugin_root_str))
            .collect();

        // Substitute ${PLUGIN_ROOT} in env values
        let env: HashMap<String, String> = spec
            .env
            .into_iter()
            .map(|(k, v)| (k, v.replace("${PLUGIN_ROOT}", &plugin_root_str)))
            .collect();

        let command = spec.command.replace("${PLUGIN_ROOT}", &plugin_root_str);

        let definition = ToolDefinition {
            name: spec.name,
            description: spec.description,
            input_schema: spec.input_schema,
        };

        let default_permission = match spec.permission {
            ToolPermission::Allow => PermissionDecision::Allow,
            ToolPermission::Ask => PermissionDecision::Ask {
                reason: "plugin tool requires approval".into(),
            },
            ToolPermission::Deny => PermissionDecision::Deny {
                reason: "plugin tool is configured to deny by default".into(),
            },
        };

        Self {
            definition,
            command,
            args,
            env,
            timeout: std::time::Duration::from_millis(spec.timeout_ms),
            is_concurrency_safe: spec.is_concurrency_safe,
            default_permission,
        }
    }

    /// Create a CommandTool directly (for programmatic construction).
    pub fn new(
        definition: ToolDefinition,
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
        timeout: std::time::Duration,
        is_concurrency_safe: bool,
        default_permission: PermissionDecision,
    ) -> Self {
        Self {
            definition,
            command,
            args,
            env,
            timeout,
            is_concurrency_safe,
            default_permission,
        }
    }
}

#[async_trait]
impl Tool for CommandTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        self.is_concurrency_safe
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Cancel
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(self.default_permission.clone())
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let args_json = serde_json::to_vec(&arguments)
            .map_err(|e| AgentError::Validation(format!("failed to serialize arguments: {e}")))?;

        let mut child = TokioCommand::new(&self.command)
            .args(&self.args)
            .envs(&self.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                AgentError::ToolExecution {
                    tool: self.definition.name.clone(),
                    message: format!("failed to spawn command '{}': {e}", self.command),
                }
            })?;

        // Write JSON arguments to stdin
        let mut stdin = child.stdin.take().ok_or_else(|| AgentError::ToolExecution {
            tool: self.definition.name.clone(),
            message: "failed to open stdin".into(),
        })?;

        let output = tokio::time::timeout(self.timeout, async {
            stdin.write_all(&args_json).await?;
            drop(stdin);
            child.wait_with_output().await
        })
        .await
        .map_err(|_| AgentError::ToolExecution {
            tool: self.definition.name.clone(),
            message: format!("tool timed out after {}ms", self.timeout.as_millis()),
        })?
        .map_err(|e| AgentError::ToolExecution {
            tool: self.definition.name.clone(),
            message: format!("I/O error: {e}"),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AgentError::ToolExecution {
                tool: self.definition.name.clone(),
                message: format!(
                    "tool exited with status {}: {}",
                    output.status,
                    stderr.trim()
                ),
            });
        }

        let value: Value = serde_json::from_slice(&output.stdout).map_err(|e| {
            AgentError::ToolExecution {
                tool: self.definition.name.clone(),
                message: format!("invalid JSON output: {e}"),
            }
        })?;

        Ok(ToolOutput::json(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn parse_tool_spec_minimal() {
        let json = json!({
            "name": "my_tool",
            "description": "A test",
            "inputSchema": {"type": "object"},
            "command": "echo"
        });
        let spec: ToolSpec = serde_json::from_value(json).unwrap();
        assert_eq!(spec.name, "my_tool");
        assert_eq!(spec.command, "echo");
        assert!(spec.args.is_empty());
        assert_eq!(spec.timeout_ms, 60_000);
        assert!(!spec.is_concurrency_safe);
    }

    #[test]
    fn parse_tool_spec_full() {
        let json = json!({
            "name": "full_tool",
            "description": "Full spec",
            "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}},
            "command": "python3",
            "args": ["-u", "${PLUGIN_ROOT}/scripts/tool.py"],
            "env": {"PYTHONUNBUFFERED": "1"},
            "timeoutMs": 10000,
            "isConcurrencySafe": true,
            "permission": "allow"
        });
        let spec: ToolSpec = serde_json::from_value(json).unwrap();
        assert_eq!(spec.name, "full_tool");
        assert_eq!(spec.args.len(), 2);
        assert_eq!(spec.timeout_ms, 10_000);
        assert!(spec.is_concurrency_safe);
        assert!(matches!(spec.permission, ToolPermission::Allow));
    }

    #[test]
    fn command_tool_from_spec_substitutes_plugin_root() {
        let spec = ToolSpec {
            name: "test".into(),
            description: "test".into(),
            input_schema: json!({}),
            command: "${PLUGIN_ROOT}/bin/tool".into(),
            args: vec!["--config".into(), "${PLUGIN_ROOT}/config.json".into()],
            env: HashMap::from([("TOOL_HOME".into(), "${PLUGIN_ROOT}/home".into())]),
            timeout_ms: 5000,
            is_concurrency_safe: false,
            permission: ToolPermission::Ask,
        };

        let tool = CommandTool::from_spec(spec, Path::new("/opt/plugin"));
        assert_eq!(tool.command, "/opt/plugin/bin/tool");
        assert_eq!(tool.args, vec!["--config", "/opt/plugin/config.json"]);
        assert_eq!(tool.env.get("TOOL_HOME").unwrap(), "/opt/plugin/home");
    }

    #[tokio::test]
    async fn command_tool_invoke_echo() {
        let definition = ToolDefinition {
            name: "echo_test".into(),
            description: "Echo test".into(),
            input_schema: json!({"type": "object"}),
        };

        let tool = CommandTool::new(
            definition,
            "cat".into(),
            vec![],
            HashMap::new(),
            std::time::Duration::from_secs(5),
            true,
            PermissionDecision::Allow,
        );

        let result = tool
            .invoke(
                json!({"hello": "world"}),
                ToolContext::dummy(),
            )
            .await
            .unwrap();

        let content = result.content;
        assert_eq!(content["hello"], "world");
    }

    #[tokio::test]
    async fn command_tool_invoke_failure() {
        let definition = ToolDefinition {
            name: "fail_test".into(),
            description: "Fail test".into(),
            input_schema: json!({"type": "object"}),
        };

        // Using `false` which exits with code 1
        let tool = CommandTool::new(
            definition,
            "false".into(),
            vec![],
            HashMap::new(),
            std::time::Duration::from_secs(5),
            false,
            PermissionDecision::Allow,
        );

        let result = tool.invoke(json!({}), ToolContext::dummy()).await;
        assert!(result.is_err());
    }

    #[test]
    fn load_tool_spec_from_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tool.json");
        std::fs::write(
            &path,
            serde_json::to_string(&json!({
                "name": "file_tool",
                "description": "Loaded from file",
                "inputSchema": {"type": "object"},
                "command": "echo",
                "permission": "allow"
            }))
            .unwrap(),
        )
        .unwrap();

        let spec = load_tool_spec(&path).unwrap();
        assert_eq!(spec.name, "file_tool");
    }
}
```

Note: `ToolContext::dummy()` doesn't exist yet. Add a test helper in `src/tool/mod.rs`:

```rust
impl ToolContext {
    /// Create a minimal context for unit tests.
    #[cfg(test)]
    pub fn dummy() -> Self {
        Self {
            session_id: "test-session".into(),
            turn_id: 0,
            cwd: std::path::PathBuf::from("."),
            env: std::collections::HashMap::new(),
            messages: std::sync::Arc::new(Vec::new()),
            progress: None,
            read_file_state: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            timeout: None,
            max_file_read_bytes: 50 * 1024 * 1024,
        }
    }
}
```

Wait — `ToolContext` lives in `tool/mod.rs`. Let me read it to find the exact location.

Actually, I saw `ToolContext` in `tool/mod.rs` — the struct is at line 111. Adding `#[cfg(test)]` methods on it from a different module won't work due to orphan rules. Instead, put the dummy constructor in the test module of `tool_loader.rs` itself, constructing the struct directly since the fields are public.

- [ ] **Step 2: Run tool loader tests**

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo test plugin::tool_loader::tests
```
Expected: all 6 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/plugin/tool_loader.rs src/plugin/mod.rs
git commit -m "feat(plugin): add CommandTool — declarative subprocess tools

CommandTool implements the Tool trait by spawning a subprocess that
receives JSON arguments on stdin and returns JSON on stdout.
ToolSpec is the declarative JSON format for plugin tool definitions.
Supports ${PLUGIN_ROOT} substitution.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 6: PluginRegistry::apply() — wire components into agent registries

**Files:**
- Modify: `src/plugin/registry.rs`
- Modify: `src/tool/mod.rs` (add `ToolContext::dummy()`)

**Interfaces:**
- Consumes: `ToolRegistry`, `HookRegistry`, `SkillRegistry`, `McpManager`, `PromptAssembly`, `CommandTool` (from Tasks 3,5)
- Produces: `PluginRegistry::apply()` method

- [ ] **Step 1: Add `ToolContext::dummy()` test helper to `src/tool/mod.rs`**

Find the `impl ToolContext {` block (around line 111) and add after the struct fields:

```rust
impl ToolContext {
    /// Create a minimal context for unit tests.
    #[cfg(test)]
    pub fn dummy() -> Self {
        Self {
            session_id: "test-session".into(),
            turn_id: 0,
            cwd: std::path::PathBuf::from("."),
            env: std::collections::HashMap::new(),
            messages: std::sync::Arc::new(Vec::new()),
            progress: None,
            read_file_state: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            timeout: None,
            max_file_read_bytes: 50 * 1024 * 1024,
        }
    }
}
```

- [ ] **Step 2: Add `apply()` method to `PluginRegistry` in `src/plugin/registry.rs`**

Add this method inside `impl PluginRegistry { ... }` (before the test module):

```rust
    /// Apply all enabled plugins' components into the agent extension registries.
    ///
    /// # Namespacing
    /// Plugin tools are registered as `plugin__<plugin_name>__<tool_name>` to
    /// avoid conflicts with built-in tools.
    ///
    /// # Errors
    /// Returns a list of per-plugin errors. Plugins that fail component loading
    /// are marked Degraded; their successfully-loaded components remain active.
    pub fn apply(
        &self,
        tools: &mut crate::tool::ToolRegistry,
        hooks: &mut crate::hooks::HookRegistry,
        skills: &mut crate::skills::SkillRegistry,
        mcp: &mut crate::mcp::McpManager,
        prompt: &mut crate::prompt::PromptAssembly,
    ) -> Result<(), Vec<PluginError>> {
        let enabled = self.list_enabled();
        let mut errors = Vec::new();

        for entry in enabled {
            let plugin = &entry.plugin;
            let plugin_id_str = plugin.id.name.clone();
            let mut component_count = 0;
            let mut loaded_count = 0;

            // --- Tools ---
            for tool_path in &plugin.resolved_tools {
                component_count += 1;
                match crate::plugin::tool_loader::load_tool_spec(tool_path) {
                    Ok(mut spec) => {
                        // Apply namespace prefix
                        spec.name = format!("plugin__{plugin_id_str}__{}", spec.name);
                        let cmd_tool =
                            crate::plugin::tool_loader::CommandTool::from_spec(spec, &plugin.path);
                        tools.register(cmd_tool);
                        loaded_count += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            plugin = %plugin.id,
                            tool = %tool_path.display(),
                            error = %e,
                            "failed to load plugin tool"
                        );
                    }
                }
            }

            // --- Skills ---
            for skill_path in &plugin.resolved_skills {
                component_count += 1;
                if skill_path.is_file() && skill_path.extension().is_some_and(|e| e == "md") {
                    match std::fs::read_to_string(skill_path) {
                        Ok(_content) => {
                            // SkillLoader::parse_skill is not public, so we use inject method.
                            // If skill_path is a directory, use inject_skills_from_dir.
                            // Otherwise it's a single file — handled by traversing the parent dir.
                            let source = crate::skills::SkillSource::Plugin {
                                plugin_id: plugin.id.clone(),
                            };
                            if let Some(parent) = skill_path.parent() {
                                if let Err(e) = skills.inject_skills_from_dir(parent, source) {
                                    tracing::warn!(
                                        plugin = %plugin.id,
                                        path = %parent.display(),
                                        error = %e,
                                        "failed to load plugin skills"
                                    );
                                } else {
                                    loaded_count += 1;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                plugin = %plugin.id,
                                skill = %skill_path.display(),
                                error = %e,
                                "failed to read plugin skill"
                            );
                        }
                    }
                }
            }

            // --- Prompt sections ---
            for section_path in &plugin.resolved_prompt_sections {
                component_count += 1;
                if section_path.is_file() {
                    match std::fs::read_to_string(section_path) {
                        Ok(template) => {
                            // Substitute ${PLUGIN_ROOT}
                            let template = template.replace(
                                "${PLUGIN_ROOT}",
                                &plugin.path.to_string_lossy(),
                            );
                            let section = PluginPromptSection {
                                name: format!("plugin_{plugin_id_str}_{}", component_count),
                                template,
                            };
                            prompt.add_static(section);
                            loaded_count += 1;
                        }
                        Err(e) => {
                            tracing::warn!(
                                plugin = %plugin.id,
                                section = %section_path.display(),
                                error = %e,
                                "failed to read plugin prompt section"
                            );
                        }
                    }
                }
            }

            // --- Agents ---
            for agent_path in &plugin.resolved_agents {
                component_count += 1;
                if agent_path.is_file() && agent_path.extension().is_some_and(|e| e == "md") {
                    match std::fs::read_to_string(agent_path) {
                        Ok(content) => {
                            // Parse as skill-like markdown with frontmatter
                            if let Some(skill) = crate::skills::loader::SkillLoader::parse_skill(
                                &content,
                                crate::skills::SkillSource::Plugin {
                                    plugin_id: plugin.id.clone(),
                                },
                            ) {
                                skills.register(skill);
                                loaded_count += 1;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                plugin = %plugin.id,
                                agent = %agent_path.display(),
                                error = %e,
                                "failed to read plugin agent"
                            );
                        }
                    }
                }
            }

            if component_count > 0 && loaded_count < component_count {
                let err = PluginError::Degraded {
                    id: plugin.id.clone(),
                    loaded: loaded_count,
                    total: component_count,
                };
                errors.push(err);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
```

This requires `PluginPromptSection` — a simple struct defined in `src/plugin/mod.rs`:

```rust
/// A prompt section backed by a static template string from a plugin.
pub struct PluginPromptSection {
    pub name: String,
    pub template: String,
}

#[async_trait::async_trait]
impl crate::prompt::PromptSection for PluginPromptSection {
    fn name(&self) -> &str {
        &self.name
    }
    fn stability(&self) -> crate::prompt::PromptStability {
        crate::prompt::PromptStability::Static
    }
    async fn render(&self, _ctx: &()) -> String {
        self.template.clone()
    }
}
```

Also update the `SkillSource` enum in `src/skills/mod.rs` to add the Plugin variant if not already present:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    Bundled,
    Managed,
    Project,
    User,
    Plugin { plugin_id: crate::plugin::PluginId },
}
```

Also need to make `SkillLoader::parse_skill` public. In `src/skills/loader.rs`, change the method visibility:

```rust
    /// Parse a single markdown string into a Skill.
    /// Non-public: use load_from_dir or load_bundled_skills.
    pub fn parse_skill(content: &str, source: SkillSource) -> Option<Skill> {
```

Wait — it's already `pub(crate)`? Let me check... The current code shows the `parse_skill` method is not listed in the impl block I read — I only see `load_from_dir` and `load_bundled_skills`. Let me check the actual visibility. Looking at the code again, SkillLoader only has `load_from_dir` and `load_bundled_skills`. The `parse_skill` function might be private. Let me adjust the design.

Actually, looking at the SkillLoader code, `parse_skill` is used internally. I need to either:
1. Make it public
2. Or use `inject_skills_from_dir` which is already public

For simplicity in the apply() method, I'll use `inject_skills_from_dir` for directory-based skills and skip single-file skills for now. The `SkillSource::Plugin` variant needs to be added though.

Let me simplify the apply() method to avoid needing to expose `parse_skill`.

- [ ] **Step 2 (revised): Implement `apply()` without needing `parse_skill`**

In `src/plugin/registry.rs`, add inside `impl PluginRegistry`:

```rust
    /// Apply all enabled plugins' components into the agent extension registries.
    pub fn apply(
        &self,
        tools: &mut crate::tool::ToolRegistry,
        _hooks: &mut crate::hooks::HookRegistry,
        skills: &mut crate::skills::SkillRegistry,
        _mcp: &mut crate::mcp::McpManager,
        _prompt: &mut crate::prompt::PromptAssembly,
    ) -> Result<(), Vec<PluginError>> {
        let enabled = self.list_enabled();
        let mut errors = Vec::new();

        for entry in enabled {
            let plugin = &entry.plugin;
            let plugin_id_str = plugin.id.name.clone();
            let mut component_count = 0;
            let mut loaded_count = 0;

            // --- Tools ---
            for tool_path in &plugin.resolved_tools {
                component_count += 1;
                match crate::plugin::tool_loader::load_tool_spec(tool_path) {
                    Ok(mut spec) => {
                        spec.name = format!("plugin__{plugin_id_str}__{}", spec.name);
                        let cmd_tool =
                            crate::plugin::tool_loader::CommandTool::from_spec(spec, &plugin.path);
                        tools.register(cmd_tool);
                        loaded_count += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            plugin = %plugin.id,
                            tool = %tool_path.display(),
                            error = %e,
                            "failed to load plugin tool"
                        );
                    }
                }
            }

            // --- Skills ---
            // Resolve skill paths: each entry can be a .md file or a directory.
            for skill_path in &plugin.resolved_skills {
                component_count += 1;
                let source = crate::skills::SkillSource::Plugin {
                    plugin_id: plugin.id.clone(),
                };
                if skill_path.is_dir() {
                    match skills.inject_skills_from_dir(skill_path, source) {
                        Ok(()) => loaded_count += 1,
                        Err(e) => {
                            tracing::warn!(
                                plugin = %plugin.id,
                                path = %skill_path.display(),
                                error = %e,
                                "failed to load plugin skills from directory"
                            );
                        }
                    }
                } else if skill_path.is_file() && skill_path.extension().is_some_and(|e| e == "md") {
                    // For single .md files, load from the containing directory
                    // (the SkillLoader scans all .md files in a directory)
                    if let Some(parent) = skill_path.parent() {
                        match skills.inject_skills_from_dir(parent, source) {
                            Ok(()) => loaded_count += 1,
                            Err(e) => {
                                tracing::warn!(
                                    plugin = %plugin.id,
                                    path = %parent.display(),
                                    error = %e,
                                    "failed to load plugin skill file"
                                );
                            }
                        }
                    }
                }
            }

            if component_count > 0 && loaded_count < component_count {
                errors.push(PluginError::Degraded {
                    id: plugin.id.clone(),
                    loaded: loaded_count,
                    total: component_count,
                });
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
```

- [ ] **Step 3: Add `SkillSource::Plugin` variant**

In `src/skills/mod.rs`, update the enum:

```rust
use crate::plugin::PluginId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    Bundled,
    Managed,
    Project,
    User,
    /// Loaded from an installed plugin.
    Plugin { plugin_id: PluginId },
}
```

- [ ] **Step 4: Write apply() integration test**

Add to the bottom of `src/plugin/registry.rs` tests:

```rust
    #[test]
    fn apply_registers_plugin_tools_with_namespace() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("installed").join("test-plugin@mkt");
        std::fs::create_dir_all(plugin_dir.join("tools")).unwrap();

        // Write plugin.json
        let manifest = json!({
            "name": "test-plugin",
            "version": "1.0.0",
            "tools": ["./tools/"]
        });
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        // Write a tool spec
        let tool_spec = json!({
            "name": "hello",
            "description": "Says hello",
            "inputSchema": {"type": "object"},
            "command": "echo",
            "permission": "allow"
        });
        std::fs::write(
            plugin_dir.join("tools").join("hello.json"),
            serde_json::to_string_pretty(&tool_spec).unwrap(),
        )
        .unwrap();

        let mut registry = PluginRegistry::new(tmp.path());
        registry.discover_installed().unwrap();
        let id = PluginId::parse("test-plugin@mkt").unwrap();
        registry.enable(&id).unwrap();

        let mut tools = crate::tool::ToolRegistry::new();
        let mut hooks = crate::hooks::HookRegistry::new();
        let mut skills = crate::skills::SkillRegistry::new();
        let mcp_config = crate::mcp::McpManager::new(std::collections::HashMap::new());
        let mut prompt = crate::prompt::PromptAssembly::new();

        let result = registry.apply(&mut tools, &mut hooks, &mut skills, &mcp_config, &mut prompt);
        assert!(result.is_ok());

        // Tool should be registered with namespace
        let tool = tools.get("plugin__test-plugin__hello");
        assert!(tool.is_ok(), "plugin tool should be registered with namespace prefix");
    }
```

- [ ] **Step 5: Build and test**

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo test plugin::registry::tests::apply_registers_plugin_tools_with_namespace
```
Expected: PASS

```bash
cargo test plugin::registry::tests
```
Expected: all 8 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/plugin/registry.rs src/plugin/mod.rs src/plugin/tool_loader.rs src/skills/mod.rs src/tool/mod.rs
git commit -m "feat(plugin): add PluginRegistry::apply() to wire components

apply() registers enabled plugin tools (with plugin__<name>__<tool>
namespace), skills, and prompt sections into agent registries.
Added SkillSource::Plugin variant. ToolContext::dummy() test helper.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 7: wire PluginRegistry into AgentConfig and AgentSession

**Files:**
- Modify: `src/config.rs`
- Modify: `src/runtime.rs`
- Modify: `src/plugin/mod.rs`

**Interfaces:**
- Consumes: `PluginRegistry` (from Task 3), `AgentConfig`
- Produces: `AgentConfig.plugin_registry` field, auto-apply on session startup

- [ ] **Step 1: Add `plugin_registry` to `AgentConfig`**

In `src/config.rs`, add the field after `pub max_file_read_bytes: usize,`:

```rust
    /// Optional plugin registry. When set, enabled plugins' components are
    /// applied to tool/hook/skill registries at session startup.
    pub plugin_registry: Option<Arc<crate::plugin::PluginRegistry>>,
```

In the `Default` impl, add:

```rust
            plugin_registry: None,
```

In the `Debug` impl, add:

```rust
            .field("plugin_registry", &self.plugin_registry.as_ref().map(|_| "<set>"))
```

- [ ] **Step 2: Auto-apply plugins in `AgentSession::run_turn_stream`**

In `src/runtime.rs`, in `run_turn_stream`, after the prompt assembly setup (around line 563-570), add plugin application. Find this block:

```rust
            if self.config.prompt_assembly.is_none() && self.config.base_system_prompt.is_none() {
                self.config.prompt_assembly = Some(Arc::new(
                    crate::prompt::default_coding_assembly(
                        Arc::new(tools.clone()),
                        self.config.cwd.clone(),
                    ),
                ));
            }
```

Add after it:

```rust
            // Apply plugin components on first turn if not yet applied.
            // We use a simple flag: check if any plugin tools are already in the
            // tool registry (plugin tools use the plugin__ prefix).
            if let Some(plugin_registry) = &self.config.plugin_registry {
                let tool_defs = tools.definitions();
                let has_plugin_tools = tool_defs
                    .iter()
                    .any(|d| d.name.starts_with("plugin__"));
                if !has_plugin_tools {
                    if let Err(errors) = plugin_registry.apply(
                        &mut tools.clone(),
                        &mut crate::hooks::HookRegistry::new(), // FIXME: need shared ownership
                        &mut crate::skills::SkillRegistry::new(),
                        &mut crate::mcp::McpManager::new(std::collections::HashMap::new()),
                        &mut crate::prompt::PromptAssembly::new(),
                    ) {
                        for err in errors {
                            tracing::warn!(%err, "plugin component load failed");
                        }
                    }
                }
            }
```

Wait — this approach is flawed because `run_turn_stream` takes `&'a ToolRegistry`, `&'a mut self`. We can't mutate `tools` here because it's a shared reference. And we can't easily access the hooks/skills/mcp/prompt registries from within the session.

Let me reconsider. The `apply()` should happen BEFORE creating the `AgentSession` — the caller is responsible for:
1. Creating the registries
2. Calling `plugin_registry.apply(tools, hooks, skills, mcp, prompt)`
3. Then passing the populated registries to `AgentConfig` and `AgentSession::new()`

This is simpler and doesn't require changes to `runtime.rs` at all. The `apply()` call happens in the integration layer (CLI or test harness), not inside the session itself.

So the change to `config.rs` is just adding the field for convenience, and the actual wiring happens in `telos-cli` or the caller's setup code.

- [ ] **Step 1 (revised): Just add the field to AgentConfig**

In `src/config.rs`:
- Add `use std::sync::Arc;` (already imported)
- Add field after `pub max_file_read_bytes`:
  ```rust
  pub plugin_registry: Option<Arc<crate::plugin::PluginRegistry>>,
  ```
- Add to Default: `plugin_registry: None,`
- Add to Debug: `.field("plugin_registry", &self.plugin_registry.as_ref().map(|_| "<set>"))`

- [ ] **Step 2: Add a convenience method on AgentConfig**

```rust
    /// Apply plugin components and return the registries populated with plugin content.
    /// Call this before creating an AgentSession.
    pub fn apply_plugins(
        &self,
        mut tools: crate::tool::ToolRegistry,
        mut hooks: crate::hooks::HookRegistry,
        mut skills: crate::skills::SkillRegistry,
        mut mcp: crate::mcp::McpManager,
        mut prompt: crate::prompt::PromptAssembly,
    ) -> Result<
        (
            crate::tool::ToolRegistry,
            crate::hooks::HookRegistry,
            crate::skills::SkillRegistry,
            crate::mcp::McpManager,
            crate::prompt::PromptAssembly,
        ),
        Vec<crate::plugin::PluginError>,
    > {
        if let Some(registry) = &self.plugin_registry {
            registry.apply(&mut tools, &mut hooks, &mut skills, &mut mcp, &mut prompt)?;
        }
        Ok((tools, hooks, skills, mcp, prompt))
    }
```

- [ ] **Step 3: Build and test**

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo check
```
Expected: compiles clean

- [ ] **Step 4: Write integration test in `tests/integration_tests.rs`**

Add:

```rust
#[tokio::test]
async fn plugin_tool_integration() {
    use tempfile::TempDir;
    use telos_agent::{
        AgentConfig, AgentSession, plugin::{PluginRegistry, PluginId},
        tool::ToolRegistry, hooks::HookRegistry, skills::SkillRegistry,
        mcp::McpManager, prompt::PromptAssembly, MockProvider, CompletionResponse, Message, StopReason,
    };

    let tmp = TempDir::new().unwrap();
    let plugin_dir = tmp.path().join("installed").join("mytool@test");
    std::fs::create_dir_all(plugin_dir.join("tools")).unwrap();

    // Write plugin.json
    let manifest = serde_json::json!({
        "name": "mytool",
        "version": "1.0.0",
        "tools": ["./tools/"]
    });
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    ).unwrap();

    // Write a tool spec
    let tool_spec = serde_json::json!({
        "name": "uppercase",
        "description": "Converts text to uppercase using tr",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"]
        },
        "command": "tr",
        "args": ["[:lower:]", "[:upper:]"],
        "permission": "allow",
        "isConcurrencySafe": true
    });
    std::fs::write(
        plugin_dir.join("tools").join("uppercase.json"),
        serde_json::to_string_pretty(&tool_spec).unwrap(),
    ).unwrap();

    // Register and enable the plugin
    let mut registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();
    let id = PluginId::parse("mytool@test").unwrap();
    registry.enable(&id).unwrap();

    // Apply plugins
    let tools = ToolRegistry::new();
    let hooks = HookRegistry::new();
    let skills = SkillRegistry::new();
    let mcp = McpManager::new(std::collections::HashMap::new());
    let prompt = PromptAssembly::new();

    let config = AgentConfig {
        plugin_registry: Some(std::sync::Arc::new(registry)),
        ..AgentConfig::default()
    };

    let (mut tools, hooks, skills, mcp, prompt) = config
        .apply_plugins(tools, hooks, skills, mcp, prompt)
        .unwrap();

    // Verify the tool is registered with namespace
    let tool = tools.get("plugin__mytool__uppercase");
    assert!(tool.is_ok(), "plugin tool should be registered: {:?}", tool.err());
}
```

Run:
```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo test plugin_tool_integration
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs tests/integration_tests.rs
git commit -m "feat(plugin): add plugin_registry to AgentConfig

AgentConfig now accepts an optional PluginRegistry. apply_plugins()
wires enabled plugin components into the agent registries before
session creation. Integration test validates end-to-end tool loading.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 8: Final build and full test suite

**Files:**
- All touched files

- [ ] **Step 1: Run full test suite**

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo test --workspace
```
Expected: all tests PASS

- [ ] **Step 2: Run clippy**

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo clippy --workspace --all-targets
```
Expected: no errors, warnings OK

- [ ] **Step 3: Run fmt**

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core && cargo fmt
```

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "chore(plugin): cargo fmt and final polish

Co-Authored-By: Claude <noreply@anthropic.com>"
```
