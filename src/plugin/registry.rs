//! PluginRegistry — manages loaded plugins and their enable/disable lifecycle.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
        Self { plugins: HashMap::new(), plugins_root: plugins_root.into() }
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
        let status = if plugin.enabled { PluginStatus::Enabled } else { PluginStatus::Disabled };
        self.plugins.insert(plugin.id.clone(), PluginEntry::new(plugin, status));
    }

    // --- Lifecycle ---

    /// Enable a plugin. Call this after registration.
    ///
    /// This is idempotent — enabling an already-enabled plugin is a no-op.
    pub fn enable(&mut self, id: &PluginId) -> Result<(), PluginError> {
        let entry = self.plugins.get_mut(id).ok_or_else(|| PluginError::PluginNotFound {
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
        let entry = self.plugins.get_mut(id).ok_or_else(|| PluginError::PluginNotFound {
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

    /// Returns `true` if no plugins are registered.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
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
        let content =
            std::fs::read_to_string(&manifest_path).map_err(|e| PluginError::ManifestParse {
                path: manifest_path.clone(),
                reason: format!("failed to read: {e}"),
            })?;

        let manifest: PluginManifest =
            serde_json::from_str(&content).map_err(|e| PluginError::ManifestParse {
                path: manifest_path.clone(),
                reason: format!("invalid JSON: {e}"),
            })?;

        if manifest.name.is_empty() {
            return Err(PluginError::ManifestValidation {
                errors: vec!["name must not be empty".into()],
            });
        }

        // Determine marketplace from the parent directory name
        // installed/<name>@<marketplace>/plugin.json
        let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("unknown@unknown");
        let id = PluginId::parse(dir_name).unwrap_or_else(|| PluginId {
            name: manifest.name.clone(),
            marketplace: "unknown".into(),
        });

        // Resolve component paths
        let resolve = |paths: &Option<Vec<String>>| -> Vec<PathBuf> {
            paths.as_ref().map(|p| p.iter().map(|s| dir.join(s)).collect()).unwrap_or_default()
        };

        // Find .json tool files in the tools/ directory
        let mut resolved_tools = resolve(&manifest.tools);
        let tools_dir = dir.join("tools");
        if tools_dir.exists()
            && tools_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&tools_dir)
        {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|ext| ext == "json") {
                    resolved_tools.push(p);
                }
            }
        }

        let resolved_skills = resolve(&manifest.skills);
        let resolved_agents = resolve(&manifest.agents);
        let resolved_prompt_sections = resolve(&manifest.prompt_sections);
        let resolved_output_styles = resolve(&manifest.output_styles);

        Ok(LoadedPlugin {
            id,
            manifest,
            path: dir.to_path_buf(),
            source: PluginSource::Local { path: dir.to_path_buf() },
            enabled: false, // will be set by state restoration
            is_builtin: false,
            resolved_tools,
            resolved_skills,
            resolved_agents,
            resolved_prompt_sections,
            resolved_output_styles,
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

        let plugins = value.get("plugins").and_then(|v| v.as_object());

        if let Some(plugins) = plugins {
            for (id_str, status_val) in plugins {
                if let Some(id) = PluginId::parse(id_str)
                    && let Some(status_str) = status_val.as_str()
                    && let Some(entry) = self.plugins.get_mut(&id)
                {
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

        Ok(())
    }

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
                let source = crate::skills::SkillSource::Plugin { plugin_id: plugin.id.clone() };
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
                } else if skill_path.is_file() && skill_path.extension().is_some_and(|e| e == "md")
                {
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

    #[test]
    fn apply_registers_plugin_tools_with_namespace() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("installed").join("test-plugin@mkt");
        std::fs::create_dir_all(plugin_dir.join("tools")).unwrap();

        // Write plugin.json
        let manifest = serde_json::json!({
            "name": "test-plugin",
            "version": "1.0.0",
            "tools": ["./tools/hello.json"]
        });
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        // Write a tool spec
        let tool_spec = serde_json::json!({
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
        let mut mcp_config = crate::mcp::McpManager::new(std::collections::HashMap::new());
        let mut prompt = crate::prompt::PromptAssembly::new();

        let result =
            registry.apply(&mut tools, &mut hooks, &mut skills, &mut mcp_config, &mut prompt);
        assert!(result.is_ok());

        // Tool should be registered with namespace
        let tool = tools.get("plugin__test-plugin__hello");
        assert!(tool.is_ok(), "plugin tool should be registered with namespace prefix");
    }
}
