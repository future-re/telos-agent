//! Plugin manifest types — serde-compatible schema for plugin.json.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::integrations::plugin::PluginId;

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
    Qualified {
        name: String,
        marketplace: String,
    },
}

impl DependencyRef {
    /// Resolve this reference into a concrete PluginId.
    ///
    /// Bare names use `default_marketplace`; qualified names use their own.
    pub fn resolve(&self, default_marketplace: &str) -> PluginId {
        match self {
            DependencyRef::Bare(name) => {
                PluginId { name: name.clone(), marketplace: default_marketplace.to_string() }
            }
            DependencyRef::Qualified { name, marketplace } => {
                PluginId { name: name.clone(), marketplace: marketplace.clone() }
            }
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

// --- Policy configuration ---

fn default_policy_timeout_ms() -> u64 {
    30_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandPolicyDef {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default = "default_policy_timeout_ms")]
    pub timeout: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPolicyDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<crate::SessionMode>,
    #[serde(flatten)]
    pub command: CommandPolicyDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicyDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    #[serde(flatten)]
    pub command: CommandPolicyDef,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoliciesConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_start: Vec<SessionPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_response: Vec<CommandPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_before_invoke: Vec<ToolPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_after_invoke: Vec<ToolPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_before_finish: Vec<CommandPolicyDef>,
}

// --- MCP configuration ---

/// MCP server configuration (mirrors crate::integrations::mcp::McpServerConfig but serde-friendly).
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

fn default_true() -> bool {
    true
}
fn default_mcp_timeout_ms() -> u64 {
    60_000
}

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

fn default_transport() -> String {
    "stdio".into()
}

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
    pub policies: Option<PoliciesConfig>,
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

fn default_strict() -> bool {
    true
}

// --- Plugin source types ---

/// Where to fetch a plugin from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginSource {
    /// Local directory containing plugin.json.
    Local { path: PathBuf },
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
        assert!(manifest.policies.is_none());
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
            "policies": {
                "toolBeforeInvoke": [{
                    "matcher": "Bash*",
                    "command": "python3",
                    "args": ["validate.py"]
                }]
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
        assert!(manifest.policies.is_some());
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
