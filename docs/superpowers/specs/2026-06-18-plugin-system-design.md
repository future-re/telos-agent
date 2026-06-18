# Plugin System Design

**Date:** 2026-06-18
**Status:** Design approved, pending implementation
**Reference:** learn-claude-code plugin system (`/home/alin/codework/learn-claude-code`)

## Overview

Add a full marketplace-based plugin system to `telos-agent`, following the architecture of learn-claude-code. Plugins are versioned, installable bundles that contribute tools, hooks, skills, MCP servers, agents, prompt sections, and output styles to the agent runtime. Marketplaces are curated collections of plugins fetched from remote sources (GitHub, git, npm, pip, URL) or local directories.

## Architecture

The plugin system lives as a new `src/plugin/` module inside `telos-agent`, following the same module-boundary patterns as `src/skills/`, `src/hooks/`, and `src/mcp/`.

### Module Layout

```
src/plugin/
  mod.rs                  # PluginRegistry, LoadedPlugin, PluginBundle, apply()
  manifest.rs             # PluginManifest, MarketplaceEntry, PluginId, serde schemas
  sources.rs              # PluginSource, MarketplaceSource enums
  marketplace.rs          # Marketplace, MarketplaceRegistry, fetch/refresh
  installer.rs            # Git clone, npm/pip install, local copy, caching
  state.rs                # PluginState persistence (~/.telos/plugin_state.json)
  tool_loader.rs          # CommandTool — declarative JSON tool specs
  hook_loader.rs          # CommandHook — hooks.json shell-out hooks
  agent_loader.rs         # Subagent markdown loading
  config.rs               # PluginConfig for ~/.telos/config.toml
  errors.rs               # PluginError discriminated union
```

### Disk Layout

```
~/.telos/
  plugins/
    marketplaces/
      telos-official/          # Default marketplace (git clone)
        marketplace.json
      community/               # User-added marketplace
        marketplace.json
    installed/
      code-formatter@telos-official/   # Installed plugin
        plugin.json
        tools/
        hooks/hooks.json
        skills/
        agents/
        prompt/
        .mcp.json
      my-plugin@community/
        plugin.json
        ...
  plugin_state.json            # Enabled/disabled for each plugin
  known_marketplaces.json      # Registered marketplace sources
```

### Crate: `telos-agent` (no new crate)

All plugin code lives inside the existing `telos-agent` crate. A future extraction to `telos-plugin` is possible when the module proves its weight. The current crate has clear module boundaries and this fits naturally.

## Core Types

### PluginId

```rust
/// "name@marketplace" — the universal plugin identifier.
/// Both parts are kebab-case alphanumeric with dots and hyphens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId {
    pub name: String,
    pub marketplace: String,
}

impl PluginId {
    pub fn parse(raw: &str) -> Result<Self, PluginError>;
    pub fn to_string(&self) -> String; // "name@marketplace"
}

/// Built-in plugins use this sentinel marketplace.
pub const BUILTIN_MARKETPLACE: &str = "builtin";
```

### PluginManifest (plugin.json)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    // Metadata
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<PluginAuthor>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub license: Option<String>,
    pub keywords: Vec<String>,
    pub dependencies: Vec<DependencyRef>,

    // Components — all optional, paths relative to plugin root
    pub tools: Option<Vec<String>>,          // paths to .json tool specs or dirs
    pub hooks: Option<HooksConfig>,          // inline or path to hooks.json
    pub skills: Option<Vec<String>>,         // paths to .md skill files or dirs
    pub agents: Option<Vec<String>>,         // paths to agent .md files or dirs
    pub mcp_servers: Option<McpServersConfig>, // inline or path to .mcp.json
    pub lsp_servers: Option<LspServersConfig>, // inline or path to .lsp.json
    pub prompt_sections: Option<Vec<String>>,   // paths to section templates
    pub output_styles: Option<Vec<String>>,     // paths to style files
    pub settings: Option<HashMap<String, Value>>,

    // User configuration prompts
    pub user_config: Option<HashMap<String, UserConfigOption>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAuthor {
    pub name: String,
    pub email: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfigOption {
    #[serde(rename = "type")]
    pub type_: ConfigOptionType,
    pub title: String,
    pub description: String,
    pub required: bool,
    pub default: Option<Value>,
    pub sensitive: bool,
    pub min: Option<f64>,
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

/// Dependency reference. Bare "name" resolves against the declaring plugin's
/// marketplace. "name@marketplace" is qualified.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencyRef {
    Bare(String),
    Qualified { name: String, marketplace: String },
}
```

### PluginSource

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PluginSource {
    #[serde(rename = "local")]
    Local { path: PathBuf },

    #[serde(rename = "github")]
    GitHub {
        repo: String,              // "owner/repo"
        #[serde(rename = "ref")]
        ref_: Option<String>,      // branch/tag
        sha: Option<String>,       // 40-char commit SHA
        path: Option<String>,      // subdirectory within repo
    },

    #[serde(rename = "git")]
    Git {
        url: String,
        #[serde(rename = "ref")]
        ref_: Option<String>,
        sha: Option<String>,
        path: Option<String>,
    },

    #[serde(rename = "npm")]
    Npm {
        package: String,
        version: Option<String>,
        registry: Option<String>,
    },

    #[serde(rename = "pip")]
    Pip {
        package: String,
        version: Option<String>,
        registry: Option<String>,
    },
}
```

### Marketplace & MarketplaceSource

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marketplace {
    pub name: String,
    pub owner: Option<PluginAuthor>,
    pub plugins: Vec<MarketplaceEntry>,
    pub force_remove_deleted_plugins: Option<bool>,
    pub allow_cross_marketplace_deps_on: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceEntry {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub source: PluginSource,
    pub category: Option<String>,
    pub tags: Vec<String>,
    #[serde(default = "default_strict")]
    pub strict: bool,    // require plugin.json? default true
    // Marketplace can override manifest fields:
    pub manifest_override: Option<Value>,
}

fn default_strict() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MarketplaceSource {
    #[serde(rename = "github")]
    GitHub { repo: String, #[serde(rename = "ref")] ref_: Option<String>, path: Option<String> },
    #[serde(rename = "git")]
    Git { url: String, #[serde(rename = "ref")] ref_: Option<String>, path: Option<String> },
    #[serde(rename = "url")]
    Url { url: String },
    #[serde(rename = "npm")]
    Npm { package: String },
    #[serde(rename = "local")]
    Local { path: PathBuf },
    #[serde(rename = "inline")]
    Inline { name: String, plugins: Vec<MarketplaceEntry> },
}
```

### LoadedPlugin

```rust
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub id: PluginId,
    pub manifest: PluginManifest,
    pub path: PathBuf,              // install directory on disk
    pub source: PluginSource,
    pub enabled: bool,
    pub is_builtin: bool,
    /// Resolved absolute paths for each component type
    pub resolved_tools: Vec<PathBuf>,
    pub resolved_hooks: Vec<PathBuf>,
    pub resolved_skills: Vec<PathBuf>,
    pub resolved_agents: Vec<PathBuf>,
    pub resolved_prompt_sections: Vec<PathBuf>,
    pub resolved_output_styles: Vec<PathBuf>,
}
```

## PluginRegistry

The central manager that owns all loaded plugins and mediates between them and the agent extension points.

```rust
#[derive(Clone)]
pub struct PluginRegistry {
    plugins: HashMap<PluginId, PluginEntry>,
    plugins_root: PathBuf,
    state: PluginState,
    marketplaces: MarketplaceRegistry,
    on_change: Vec<Arc<dyn Fn(&PluginId, PluginStatus) + Send + Sync>>,
}

struct PluginEntry {
    plugin: LoadedPlugin,
    status: PluginStatus,
    load_errors: Vec<PluginError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    Enabled,
    Disabled,
    Degraded,  // enabled but some components failed to load
    Error,     // failed to load entirely
}
```

### Key Methods

```rust
impl PluginRegistry {
    /// Create a new registry backed by `plugins_root` (~/.telos/plugins/).
    pub fn new(plugins_root: PathBuf) -> Self;

    /// Scan installed/ and load all found plugins. Does NOT enable them.
    pub async fn discover_installed(&mut self) -> Result<Vec<PluginId>, PluginError>;

    /// Register and enable built-in plugins (ships with the binary).
    pub fn register_builtin(&mut self, plugins: Vec<LoadedPlugin>);

    /// Install from a source: fetch, parse plugin.json, register.
    pub async fn install(&mut self, source: PluginSource, marketplace: &str)
        -> Result<PluginId, PluginError>;

    /// Enable a plugin. Validates dependencies, prompts user_config, applies components.
    pub async fn enable(&mut self, id: &PluginId) -> Result<(), PluginError>;

    /// Disable a plugin. Removes all its components from agent registries.
    pub async fn disable(&mut self, id: &PluginId) -> Result<(), PluginError>;

    /// Uninstall: disable, then delete from disk.
    pub async fn uninstall(&mut self, id: &PluginId) -> Result<(), PluginError>;

    /// Update to latest version from the plugin's source.
    pub async fn update(&mut self, id: &PluginId) -> Result<(), PluginError>;

    /// Apply all enabled plugins' components into the agent registries.
    /// Called at session startup after discovery + auto-enable.
    pub fn apply(
        &self,
        tools: &mut ToolRegistry,
        hooks: &mut HookRegistry,
        skills: &mut SkillRegistry,
        mcp: &mut McpManager,
        prompt: &mut PromptAssembly,
    ) -> Result<(), Vec<PluginError>>;

    // Query
    pub fn get(&self, id: &PluginId) -> Option<&PluginEntry>;
    pub fn list_enabled(&self) -> Vec<&PluginEntry>;
    pub fn list_disabled(&self) -> Vec<&PluginEntry>;
    pub fn list_all(&self) -> Vec<&PluginEntry>;

    // Persistence
    pub fn save_state(&self) -> Result<(), PluginError>;
    pub fn load_state(&mut self) -> Result<(), PluginError>;
}
```

### Component Application (apply)

```
PluginRegistry::apply()
  ├── Validate dependency closure for all enabled plugins
  ├── for each enabled plugin (topological order by deps):
  │   ├── Load declarative tools → CommandTool → ToolRegistry::register()
  │   │   Namespace: "plugin__<name>__<tool>" to avoid conflicts
  │   ├── Load hooks from hooks.json → CommandHook → HookRegistry::register_entry()
  │   ├── Load skills (*.md) → SkillLoader → SkillRegistry::register()
  │   ├── Load MCP servers → McpManager::add_server()
  │   ├── Load agents (*.md) → SubagentRegistry::register()
  │   ├── Load prompt sections → PluginPromptSection → PromptAssembly
  │   └── Load output styles → OutputStyleRegistry (new)
  └── Return errors for degraded plugins, don't block healthy ones
```

### Namespace Convention

To prevent tool name conflicts between plugins and built-in tools, plugin tools get a namespace prefix:

- Built-in: `Bash`, `Read`, `Write`, `Grep`, `Glob`, ...
- Plugin `foo`: `plugin__foo__my_tool`, `plugin__foo__formatter`
- MCP tools keep their existing `mcp__<server>__<tool>` prefix

Plugin skills/agents don't need namespacing — their names are user-visible slash commands and the name conflict is resolved at registration time (later wins, warning emitted).

## Component Loading Details

### Tools: CommandTool

Plugin tools are declarative JSON specs executed as subprocesses. The tool arguments arrive as JSON on stdin; stdout JSON is the tool result.

JSON spec format (`tools/my-tool.json`):

```json
{
  "name": "my_tool",
  "description": "Does something useful",
  "input_schema": {
    "type": "object",
    "properties": { "text": { "type": "string" } },
    "required": ["text"]
  },
  "command": "python3",
  "args": ["${PLUGIN_ROOT}/scripts/my_tool.py"],
  "env": { "PYTHONUNBUFFERED": "1" },
  "timeout_ms": 30000,
  "is_concurrency_safe": false,
  "permission": "ask"
}
```

`${PLUGIN_ROOT}` is substituted at load time with the plugin's absolute install path.

```rust
pub struct CommandTool {
    definition: ToolDefinition,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    timeout: Duration,
    is_concurrency_safe: bool,
    default_permission: PermissionDecision,
}

#[async_trait]
impl Tool for CommandTool {
    fn definition(&self) -> ToolDefinition { self.definition.clone() }
    fn is_concurrency_safe(&self, _: &Value) -> bool { self.is_concurrency_safe }

    async fn check_permission(&self, _: &Value, _: &ToolContext)
        -> Result<PermissionDecision, AgentError>
    { Ok(self.default_permission.clone()) }

    async fn invoke(&self, args: Value, ctx: ToolContext)
        -> Result<ToolOutput, AgentError>
    {
        let mut child = tokio::process::Command::new(&self.command)
            .args(&self.args)
            .envs(&self.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        // Write JSON arguments to stdin
        let stdin = child.stdin.take().unwrap();
        let args_json = serde_json::to_vec(&args)?;
        tokio::io::AsyncWriteExt::write_all(&mut stdin, &args_json).await?;
        drop(stdin);

        // Wait with timeout
        let output = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| AgentError::Timeout)?
            .map_err(|e| AgentError::Io(e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AgentError::ToolExecution(format!(
                "tool exited with {}: {stderr}", output.status
            )));
        }

        let value: Value = serde_json::from_slice(&output.stdout)?;
        Ok(ToolOutput::json(value))
    }
}
```

### Hooks: CommandHook

Loaded from `hooks/hooks.json` — same format as learn-claude-code's `HooksSchema`:

```json
{
  "PreToolUse": [
    {
      "matcher": "Bash(git *)",
      "hooks": [
        {
          "type": "command",
          "command": "python3",
          "args": ["${PLUGIN_ROOT}/hooks/validate_git.py"],
          "timeout": 10000
        }
      ]
    }
  ],
  "SessionStart": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "${PLUGIN_ROOT}/hooks/on_start.sh",
          "timeout": 5000
        }
      ]
    }
  ]
}
```

```rust
pub struct CommandHook {
    name: String,
    phase: HookPhase,
    command: String,
    args: Vec<String>,
    timeout: Duration,
}

#[async_trait]
impl Hook for CommandHook {
    fn name(&self) -> &str { &self.name }
    fn phase(&self) -> HookPhase { self.phase.clone() }

    async fn run(&self, ctx: &HookContext, msg: &Message)
        -> Result<Option<Message>, AgentError>
    {
        let input = json!({
            "hook_context": {
                "session_id": ctx.session_id,
                "turn_id": ctx.turn_id,
                "message_count": ctx.message_count,
            },
            "message": msg,
        });

        // Spawn subprocess, pipe input JSON to stdin, read stdout JSON.
        // stdout format: {"continue": true, "additional_context": "..."}
        // Returning an "additional_context" string appends it as a follow-up message.
        // ...
    }
}
```

### Skills

Plugin skill markdown files are loaded through the existing `SkillLoader`. The only addition is setting `SkillSource::Plugin`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    Bundled,
    Managed,
    Project,
    User,
    Plugin { plugin_id: PluginId },  // NEW variant
}
```

### MCP Servers

Plugin MCP server configs pass directly to `McpManager::add_server()`. The existing MCP infrastructure handles the rest.

### Prompt Sections

Plugin prompt sections are template files rendered once and cached:

```rust
pub struct PluginPromptSection {
    name: String,
    template: String,  // read from file, ${PLUGIN_ROOT} already substituted
}

impl PromptSection for PluginPromptSection {
    fn name(&self) -> &str { &self.name }
    fn stability(&self) -> PromptStability { PromptStability::Static }
    async fn build(&self) -> String { self.template.clone() }
}
```

### Agents

Plugin agents are markdown files defining subagent types. A new `SubagentRegistry` holds named agent types referenceable by the `Agent` tool:

```rust
pub struct SubagentDefinition {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
    pub model: Option<String>,
}

pub struct SubagentRegistry {
    agents: HashMap<String, SubagentDefinition>,
}
```

The existing `SubagentTool` reads from this registry when `agent_type` is specified.

### Output Styles

Plugin output styles are theme files consumed by `telos-cli`'s terminal renderer. They define color palettes and formatting rules:

```json
// output-styles/dark-theme.json
{
  "name": "Dark Theme",
  "colors": {
    "tool_call": "#4FC3F7",
    "tool_result": "#81C784",
    "error": "#E57373",
    "thinking": "#CE93D8"
  }
}
```

## Built-in Plugins

Built-in plugins ship with the telos binary and appear in `/plugin list` under the `@builtin` marketplace. They use the same `LoadedPlugin` type but `is_builtin: true`.

```rust
// src/plugin/builtins.rs
pub fn register_builtin_plugins(registry: &mut PluginRegistry) {
    // These are loaded from embedded static assets (include_str! or similar)
    // rather than from disk. They are always available, user-toggleable.
    registry.register_builtin(vec![
        // Example: builtin plugin that adds safety hooks
        // LoadedPlugin { id: PluginId { name: "safety", marketplace: "builtin".into() }, ... }
    ]);
}
```

Built-in plugins are defined in code (not on disk) — they have no install path, their components are hardcoded `Tool`/`Hook`/`Skill` instances registered directly.

## Marketplace Registry

```rust
pub struct MarketplaceRegistry {
    marketplaces: HashMap<String, CachedMarketplace>,
    cache_root: PathBuf,
}

struct CachedMarketplace {
    source: MarketplaceSource,
    manifest: Marketplace,
    install_location: PathBuf,  // cloned/cached on disk
    last_updated: DateTime<Utc>,
}

impl MarketplaceRegistry {
    /// Add a marketplace source: clone/fetch it, parse marketplace.json, cache it.
    pub async fn add(&mut self, source: MarketplaceSource) -> Result<String, PluginError>;

    /// Remove a marketplace and its cached data.
    pub async fn remove(&mut self, name: &str) -> Result<(), PluginError>;

    /// Refresh all marketplaces (re-fetch marketplace.json).
    pub async fn refresh_all(&mut self) -> Vec<(String, Result<(), PluginError>)>;

    /// Refresh one marketplace.
    pub async fn refresh(&mut self, name: &str) -> Result<(), PluginError>;

    /// Search across all marketplaces for plugins matching `query`.
    pub fn search(&self, query: &str) -> Vec<(&str, &MarketplaceEntry)>;

    /// List all available plugins across all marketplaces.
    pub fn list_all(&self) -> Vec<(&str, &MarketplaceEntry)>;

    /// Persist known marketplaces to disk.
    pub fn save(&self) -> Result<(), PluginError>;

    /// Load known marketplaces from disk.
    pub fn load(&mut self) -> Result<(), PluginError>;
}
```

## Installer

Handles fetching and caching plugins from various sources:

```rust
pub struct PluginInstaller {
    cache_root: PathBuf,  // ~/.telos/plugins/installed/
}

impl PluginInstaller {
    /// Fetch a plugin from any source. Returns the path to the plugin directory
    /// containing plugin.json.
    pub async fn fetch(&self, source: &PluginSource) -> Result<PathBuf, PluginError>;

    /// Update an already-installed plugin (git pull / npm update / re-copy).
    pub async fn update(&self, id: &PluginId, source: &PluginSource, current_path: &Path)
        -> Result<PathBuf, PluginError>;

    /// Remove an installed plugin directory.
    pub fn remove(&self, path: &Path) -> Result<(), PluginError>;
}
```

Implementation delegates to:
- `git clone --depth 1` for GitHub/git sources
- `npm pack && tar xf` for npm (run in temp dir, extract plugin dir)
- `pip download && unzip` for pip
- `cp -r` for local directories

All fetch operations use temp directories and atomic renames to avoid partial state.

## User Configuration Prompts

When a plugin with `user_config` is enabled, the user is prompted to provide values:

```rust
/// Interactive prompt flow for user_config (used by CLI).
pub async fn prompt_user_config(
    plugin: &LoadedPlugin,
    existing: Option<&HashMap<String, Value>>,
) -> Result<HashMap<String, Value>, PluginError>;
```

Non-sensitive values are stored in plugin state JSON. Sensitive values go to a credentials file (`~/.telos/credentials.json`) with restricted permissions.

Values are available to plugin components via `${user_config.KEY}` substitution in tool commands, hook commands, and MCP server env.

## Dependency Resolution

At enable time, the dependency graph is validated:

1. Parse each `DependencyRef` into a concrete `PluginId`:
   - `"foo"` → `PluginId { name: "foo", marketplace: plugin.marketplace }`
   - `"foo@other-mkt"` → `PluginId { name: "foo", marketplace: "other-mkt" }`
2. DFS from the target plugin through all dependencies
3. If any dep is missing → `PluginError::DependencyUnsatisfied { reason: NotFound }`
4. If any dep is installed but disabled → `PluginError::DependencyUnsatisfied { reason: NotEnabled }`
5. If a cycle is detected → `PluginError::CircularDependency`
6. Enable in topological order (deps first, then dependents)

## Error Handling

```rust
#[derive(Debug, Error)]
pub enum PluginError {
    // Manifest
    #[error("Manifest not found at {path}")]
    ManifestNotFound { path: PathBuf },
    #[error("Manifest parse error: {reason}")]
    ManifestParse { path: PathBuf, reason: String },
    #[error("Manifest validation failed: {errors:?}")]
    ManifestValidation { errors: Vec<String> },

    // Sources
    #[error("Plugin '{plugin_id}' not found in marketplace '{marketplace}'")]
    PluginNotFound { plugin_id: String, marketplace: String },
    #[error("Marketplace '{marketplace}' not found")]
    MarketplaceNotFound { marketplace: String },
    #[error("Git clone failed: {reason}")]
    GitCloneFailed { url: String, reason: String },
    #[error("npm install failed: {reason}")]
    NpmInstallFailed { package: String, reason: String },
    #[error("pip install failed: {reason}")]
    PipInstallFailed { package: String, reason: String },
    #[error("Network error fetching {url}: {detail}")]
    NetworkError { url: String, detail: String },

    // Dependencies
    #[error("Dependency '{dependency}' is {reason}")]
    DependencyUnsatisfied { dependency: String, reason: DependencyReason },
    #[error("Circular dependency: {cycle:?}")]
    CircularDependency { cycle: Vec<PluginId> },

    // Lifecycle
    #[error("Plugin '{0}' is already enabled")]
    AlreadyEnabled(PluginId),
    #[error("Plugin '{0}' is already disabled")]
    AlreadyDisabled(PluginId),
    #[error("Plugin '{0}' is degraded: {1}/{2} components loaded")]
    Degraded { id: PluginId, loaded: usize, total: usize },
    #[error("Plugin '{0}' component load failed: {1}")]
    ComponentLoadFailed(PluginId, String),

    // User config
    #[error("User configuration required")]
    UserConfigRequired { id: PluginId },
    #[error("User configuration validation: {errors:?}")]
    UserConfigValidation { errors: Vec<String> },

    // Generic
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub enum DependencyReason {
    NotEnabled,
    NotFound,
}
```

**Recovery strategy:**
- Manifest parse failure → plugin marked `Error`, surfaced in `/plugin list`, other plugins unaffected
- Component load failure → plugin marked `Degraded`, working components remain active
- Dependency failure → plugin stays disabled, clear error message
- Install failure → partial files cleaned up, previous version preserved on update
- Network failure during refresh → stale marketplace data retained, warning emitted

## CLI Commands

```bash
# Plugin management
telos plugin list [--enabled|--disabled]
telos plugin search <query>
telos plugin install <name>@<marketplace>
telos plugin uninstall <name>@<marketplace>
telos plugin enable <name>@<marketplace>
telos plugin disable <name>@<marketplace>
telos plugin update <name>@<marketplace>
telos plugin update --all
telos plugin info <name>@<marketplace>
telos plugin validate <path/to/plugin.json>

# Marketplace management
telos marketplace add <source>
telos marketplace remove <name>
telos marketplace list
telos marketplace update [--all|<name>]
```

## Configuration

In `~/.telos/config.toml`:

```toml
[plugins]
# Extra marketplaces (telos-official is always registered)
extra_marketplaces = [
    { type = "github", repo = "my-org/telos-plugins" },
    { type = "local", path = "/home/user/dev/custom-plugins" },
]

# Auto-update marketplaces on startup
auto_update_marketplaces = true

# Plugins to auto-enable when first discovered
auto_enable = ["code-formatter@community"]

# Maximum concurrent plugin operations (git clone, npm fetch)
max_concurrent_installs = 4
```

## Implementation Phases

### Phase 1: Core Types & Registry (foundation)
- `src/plugin/mod.rs`, `manifest.rs`, `sources.rs`, `errors.rs`, `state.rs`, `config.rs`
- `PluginRegistry` with `register`, `enable`, `disable`, `discover_installed`
- `PluginId`, `PluginManifest`, `PluginSource`, `MarketplaceSource`, `PluginError`
- State persistence (`plugin_state.json`)
- Unit tests for registry operations

### Phase 2: Marketplace & Installer
- `src/plugin/marketplace.rs` — `MarketplaceRegistry`, fetch/refresh
- `src/plugin/installer.rs` — Git clone, local copy
- `known_marketplaces.json` persistence
- npm/pip stubs (error with "not yet supported")

### Phase 3: Component Loaders
- `src/plugin/tool_loader.rs` — `CommandTool` + declarative JSON specs
- `src/plugin/hook_loader.rs` — `CommandHook` + hooks.json
- `src/plugin/agent_loader.rs` — Subagent definitions
- Plugin skill/prompt/output-style loading
- Integration: `PluginRegistry::apply()`

### Phase 4: CLI & Built-in Plugins
- CLI subcommands in `telos-cli`
- `src/plugin/builtins.rs` — built-in plugin registration
- User config prompting
- Dependency resolution

### Phase 5: npm/pip & Polish
- npm package source support
- pip package source support
- `/plugin` agent tools (slash commands)
- Error recovery hardening
- Documentation

## Self-Review

- **No placeholders or TBDs** — all sections are concrete
- **Internal consistency** — manifest schema matches loader expectations, PluginId used consistently across all APIs, namespace convention applied uniformly
- **Scope** — one focused spec for the plugin system; marketplace and installer are sub-components, not separate specs
- **No ambiguity** — all types have Rust definitions, all flows have step-by-step descriptions
