//! Plugin manifest types — serde-compatible schema for plugin.json.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::integrations::plugin::PluginId;

// --- Metadata types ---

/// Author or maintainer of a plugin or marketplace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginAuthor {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// One user-configurable option declared by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

/// A versioned dependency reference. Omitting `marketplace` resolves against
/// the declaring plugin's marketplace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyRef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marketplace: Option<String>,
    pub version: semver::VersionReq,
}

impl DependencyRef {
    /// Resolve this reference into a concrete PluginId.
    ///
    /// Bare names use `default_marketplace`; qualified names use their own.
    pub fn resolve(&self, default_marketplace: &str) -> PluginId {
        PluginId {
            name: self.name.clone(),
            marketplace: self
                .marketplace
                .clone()
                .unwrap_or_else(|| default_marketplace.to_string()),
        }
    }

    /// Display the dependency as a string.
    pub fn display(&self) -> String {
        self.marketplace
            .as_ref()
            .map_or_else(|| self.name.clone(), |marketplace| format!("{}@{marketplace}", self.name))
    }
}

// --- Policy configuration ---

fn default_policy_timeout_ms() -> u64 {
    30_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandPolicyDef {
    /// Stable identifier used in lifecycle events. Generated from declaration order when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PoliciesConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_start: Vec<SessionPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_end: Vec<CommandPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_start: Vec<CommandPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_before_request: Vec<CommandPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_response: Vec<CommandPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_before_invoke: Vec<ToolPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_after_invoke: Vec<ToolPolicyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_before_finish: Vec<CommandPolicyDef>,
}

impl PoliciesConfig {
    pub(crate) fn validate(&self) -> Vec<String> {
        fn validate_command(path: &str, policy: &CommandPolicyDef, errors: &mut Vec<String>) {
            if policy.name.as_ref().is_some_and(|name| name.trim().is_empty()) {
                errors.push(format!("{path}.name must not be empty"));
            }
            if policy.command.trim().is_empty() {
                errors.push(format!("{path}.command must not be empty"));
            }
            if policy.timeout == 0 {
                errors.push(format!("{path}.timeout must be greater than zero"));
            }
        }

        let mut errors = Vec::new();

        for (index, policy) in self.session_start.iter().enumerate() {
            validate_command(
                &format!("policies.sessionStart[{index}]"),
                &policy.command,
                &mut errors,
            );
        }
        for (index, policy) in self.session_end.iter().enumerate() {
            validate_command(&format!("policies.sessionEnd[{index}]"), policy, &mut errors);
        }
        for (index, policy) in self.turn_start.iter().enumerate() {
            validate_command(&format!("policies.turnStart[{index}]"), policy, &mut errors);
        }
        for (index, policy) in self.model_before_request.iter().enumerate() {
            validate_command(&format!("policies.modelBeforeRequest[{index}]"), policy, &mut errors);
        }
        for (index, policy) in self.model_response.iter().enumerate() {
            validate_command(&format!("policies.modelResponse[{index}]"), policy, &mut errors);
        }
        for (index, policy) in self.tool_before_invoke.iter().enumerate() {
            let path = format!("policies.toolBeforeInvoke[{index}]");
            validate_command(&path, &policy.command, &mut errors);
            if let Some(matcher) = &policy.matcher
                && let Err(error) = glob::Pattern::new(matcher)
            {
                errors.push(format!("{path}.matcher is invalid: {error}"));
            }
        }
        for (index, policy) in self.tool_after_invoke.iter().enumerate() {
            let path = format!("policies.toolAfterInvoke[{index}]");
            validate_command(&path, &policy.command, &mut errors);
            if let Some(matcher) = &policy.matcher
                && let Err(error) = glob::Pattern::new(matcher)
            {
                errors.push(format!("{path}.matcher is invalid: {error}"));
            }
        }
        for (index, policy) in self.turn_before_finish.iter().enumerate() {
            validate_command(&format!("policies.turnBeforeFinish[{index}]"), policy, &mut errors);
        }
        errors
    }
}

// --- MCP configuration ---

/// MCP server configuration (mirrors crate::integrations::mcp::McpServerConfig but serde-friendly).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifest {
    #[serde(deserialize_with = "deserialize_manifest_version")]
    pub manifest_version: u32,
    #[serde(default)]
    pub name: String,
    pub version: semver::Version,
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

fn deserialize_manifest_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let version = u32::deserialize(deserializer)?;
    if version == 2 {
        Ok(version)
    } else {
        Err(serde::de::Error::custom(format!("unsupported manifestVersion {version}; expected 2")))
    }
}

/// A partial manifest — marketplace entries can override fields.
///
/// This is a subset of PluginManifest with all optional fields.
pub type PartialPluginManifest = Value;

/// An entry in a marketplace — describes a plugin and where to get it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarketplaceEntry {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: semver::Version,
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
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
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
            "manifestVersion": 2,
            "name": "my-plugin",
            "version": "1.0.0",
            "description": "A test plugin"
        });
        let manifest: PluginManifest = serde_json::from_value(json).unwrap();
        assert_eq!(manifest.name, "my-plugin");
        assert_eq!(manifest.version, semver::Version::new(1, 0, 0));
        assert!(manifest.tools.is_none());
        assert!(manifest.policies.is_none());
        assert!(manifest.dependencies.is_empty());
    }

    #[test]
    fn manifest_v2_requires_versioned_object_dependencies() {
        let v1 = serde_json::from_value::<PluginManifest>(json!({
            "manifestVersion": 1,
            "name": "legacy",
            "version": "1.0.0"
        }));
        assert!(v1.is_err());

        let string_dependency = serde_json::from_value::<PluginManifest>(json!({
            "manifestVersion": 2,
            "name": "modern",
            "version": "1.0.0",
            "dependencies": ["legacy"]
        }));
        assert!(string_dependency.is_err());

        let missing_version = serde_json::from_value::<PluginManifest>(json!({
            "manifestVersion": 2,
            "name": "modern",
            "version": "1.0.0",
            "dependencies": [{"name": "dep"}]
        }));
        assert!(missing_version.is_err());
    }

    #[test]
    fn rejects_unknown_nested_manifest_fields() {
        let author_error = serde_json::from_value::<PluginManifest>(json!({
            "manifestVersion": 2,
            "name": "strict",
            "version": "1.0.0",
            "author": {"name": "Alice", "emali": "typo@example.com"}
        }))
        .unwrap_err();
        assert!(author_error.to_string().contains("unknown field"));

        let policy_error = serde_json::from_value::<PluginManifest>(json!({
            "manifestVersion": 2,
            "name": "strict",
            "version": "1.0.0",
            "policies": {
                "turnStart": [{"command": "echo", "argz": []}]
            }
        }))
        .unwrap_err();
        assert!(policy_error.to_string().contains("unknown field"));

        let dependency_error = serde_json::from_value::<PluginManifest>(json!({
            "manifestVersion": 2,
            "name": "strict",
            "version": "1.0.0",
            "dependencies": [{"name": "dep", "marketplace": "mkt", "version": "^1", "marketpalce": "typo"}]
        }))
        .unwrap_err();
        assert!(dependency_error.to_string().contains("unknown field"));
    }

    #[test]
    fn parse_full_manifest() {
        let json = json!({
            "manifestVersion": 2,
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
                {"name": "required-dep", "version": "^1"},
                {"name": "other", "marketplace": "community", "version": ">=2, <3"}
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
        assert_eq!(manifest.version, semver::Version::new(2, 1, 0));
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
    fn parse_dependency_ref_default_marketplace() {
        let json = json!({"name": "my-dep", "version": "^1.2"});
        let dep: DependencyRef = serde_json::from_value(json).unwrap();
        assert_eq!(dep.display(), "my-dep");
        let id = dep.resolve("my-marketplace");
        assert_eq!(id.to_string(), "my-dep@my-marketplace");
    }

    #[test]
    fn policy_validation_rejects_invalid_commands_timeouts_and_matchers() {
        let policies: PoliciesConfig = serde_json::from_value(json!({
            "modelResponse": [{"name": "", "command": "", "timeout": 0}],
            "toolBeforeInvoke": [{"matcher": "[", "command": "check"}]
        }))
        .unwrap();

        let errors = policies.validate();
        assert!(errors.iter().any(|error| error.contains("name must not be empty")));
        assert!(errors.iter().any(|error| error.contains("command must not be empty")));
        assert!(errors.iter().any(|error| error.contains("timeout must be greater than zero")));
        assert!(errors.iter().any(|error| error.contains("matcher is invalid")));
    }

    #[test]
    fn parses_all_semantic_policy_boundaries() {
        let policies: PoliciesConfig = serde_json::from_value(json!({
            "sessionStart": [{"command": "start"}],
            "sessionEnd": [{"command": "end"}],
            "turnStart": [{"command": "turn"}],
            "modelBeforeRequest": [{"command": "before-model"}],
            "modelResponse": [{"command": "after-model"}],
            "toolBeforeInvoke": [{"command": "before-tool"}],
            "toolAfterInvoke": [{"command": "after-tool"}],
            "turnBeforeFinish": [{"command": "finish"}]
        }))
        .unwrap();

        assert_eq!(policies.session_start.len(), 1);
        assert_eq!(policies.session_end.len(), 1);
        assert_eq!(policies.turn_start.len(), 1);
        assert_eq!(policies.model_before_request.len(), 1);
        assert_eq!(policies.model_response.len(), 1);
        assert_eq!(policies.tool_before_invoke.len(), 1);
        assert_eq!(policies.tool_after_invoke.len(), 1);
        assert_eq!(policies.turn_before_finish.len(), 1);
        assert!(policies.validate().is_empty());
    }

    #[test]
    fn parse_dependency_ref_qualified() {
        let json = json!({"name": "dep", "marketplace": "other-mkt", "version": "=2.0.0"});
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
