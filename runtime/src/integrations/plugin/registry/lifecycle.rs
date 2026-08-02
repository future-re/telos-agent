//! Plugin registry lifecycle and query methods.

use crate::integrations::plugin::ResolvedPluginConfig;
use crate::integrations::plugin::config::PluginConfigStore;
use crate::integrations::plugin::registry::types::{LoadedPlugin, PluginEntry, PluginStatus};
use crate::integrations::plugin::{DependencyReason, PluginError, PluginId};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

pub struct PluginRegistry {
    pub(crate) plugins: RwLock<HashMap<PluginId, PluginEntry>>,
    pub(crate) plugins_root: PathBuf,
    pub(crate) config_store: RwLock<PluginConfigStore>,
}

impl PluginRegistry {
    /// Create a new registry backed by `plugins_root` (typically `~/.telos/plugins/`).
    pub(crate) fn new(plugins_root: impl Into<PathBuf>) -> Self {
        let plugins_root = plugins_root.into();
        let config_store = PluginConfigStore::new(plugins_root.join("state.json"));
        Self {
            plugins: RwLock::new(HashMap::new()),
            plugins_root,
            config_store: RwLock::new(config_store),
        }
    }
    /// Path where installed plugins live.
    pub(crate) fn installed_dir(&self) -> PathBuf {
        self.plugins_root.join("installed")
    }

    /// Path to the state file.
    pub(crate) fn state_path(&self) -> PathBuf {
        self.plugins_root.join("state.json")
    }

    pub(crate) fn load_config(&self) -> Result<(), PluginError> {
        self.config_store.write().expect("plugin config lock poisoned").load()
    }

    pub(crate) fn set_config(
        &self,
        id: &PluginId,
        values: HashMap<String, Value>,
    ) -> Result<(), PluginError> {
        let manifest = self
            .plugins
            .read()
            .expect("plugin registry lock poisoned")
            .get(id)
            .ok_or_else(|| PluginError::PluginNotFound {
                plugin_id: id.to_string(),
                marketplace: id.marketplace.clone(),
            })?
            .plugin
            .manifest
            .clone();
        self.config_store.write().expect("plugin config lock poisoned").set(id, &manifest, values)
    }

    pub(crate) fn clear_config(&self, id: &PluginId) -> Result<(), PluginError> {
        self.config_store.write().expect("plugin config lock poisoned").clear(id)
    }

    pub fn resolved_config(&self, id: &PluginId) -> Result<ResolvedPluginConfig, PluginError> {
        let manifest = self
            .plugins
            .read()
            .expect("plugin registry lock poisoned")
            .get(id)
            .ok_or_else(|| PluginError::PluginNotFound {
                plugin_id: id.to_string(),
                marketplace: id.marketplace.clone(),
            })?
            .plugin
            .manifest
            .clone();
        self.config_store.read().expect("plugin config lock poisoned").resolve(id, &manifest)
    }

    pub(crate) fn validate_config_for_manifest(
        &self,
        id: &PluginId,
        manifest: &crate::integrations::plugin::PluginManifest,
    ) -> Result<(), PluginError> {
        self.config_store
            .read()
            .expect("plugin config lock poisoned")
            .validate_for_manifest(id, manifest)
    }
    /// Register a loaded plugin without enabling it.
    ///
    /// If a plugin with the same ID already exists, it is replaced
    /// (the old plugin's state is lost).
    pub fn register(&self, plugin: LoadedPlugin) {
        let status = if plugin.enabled { PluginStatus::Enabled } else { PluginStatus::Disabled };
        self.plugins
            .write()
            .expect("plugin registry lock poisoned")
            .insert(plugin.id.clone(), PluginEntry::new(plugin, status));
    }
    /// Enable a plugin. Call this after registration.
    ///
    /// This is idempotent — enabling an already-enabled plugin is a no-op.
    pub(crate) fn enable(&self, id: &PluginId) -> Result<(), PluginError> {
        self.resolved_config(id)?;
        self.validate_dependencies(id)?;
        let mut plugins = self.plugins.write().expect("plugin registry lock poisoned");
        let entry = plugins.get_mut(id).ok_or_else(|| PluginError::PluginNotFound {
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
    pub(crate) fn disable(&self, id: &PluginId) -> Result<(), PluginError> {
        let mut plugins = self.plugins.write().expect("plugin registry lock poisoned");
        let dependents =
            plugins
                .values()
                .filter(|entry| {
                    entry.plugin.id != *id
                        && matches!(entry.status, PluginStatus::Enabled | PluginStatus::Degraded)
                        && entry.plugin.manifest.dependencies.iter().any(|dependency| {
                            dependency.resolve(&entry.plugin.id.marketplace) == *id
                        })
                })
                .map(|entry| entry.plugin.id.clone())
                .collect::<Vec<_>>();
        if !dependents.is_empty() {
            return Err(PluginError::DependencyRequiredBy { dependency: id.clone(), dependents });
        }
        let entry = plugins.get_mut(id).ok_or_else(|| PluginError::PluginNotFound {
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
    pub fn mark_degraded(&self, id: &PluginId, errors: Vec<PluginError>) {
        if let Some(entry) =
            self.plugins.write().expect("plugin registry lock poisoned").get_mut(id)
        {
            entry.status = PluginStatus::Degraded;
            entry.load_errors = errors;
        }
    }
    /// Mark a plugin as in error state.
    pub fn mark_error(&self, id: &PluginId, error: PluginError) {
        if let Some(entry) =
            self.plugins.write().expect("plugin registry lock poisoned").get_mut(id)
        {
            entry.status = PluginStatus::Error;
            entry.load_errors = vec![error];
        }
    }
    /// Mark a previously degraded/error plugin as fully active after a clean apply.
    pub fn mark_loaded(&self, id: &PluginId) {
        if let Some(entry) =
            self.plugins.write().expect("plugin registry lock poisoned").get_mut(id)
        {
            entry.status = PluginStatus::Enabled;
            entry.plugin.enabled = true;
            entry.load_errors.clear();
        }
    }
    /// Remove a plugin from the registry entirely.
    pub fn remove(&self, id: &PluginId) -> Option<PluginEntry> {
        self.plugins.write().expect("plugin registry lock poisoned").remove(id)
    }
    /// Look up a plugin by ID.
    pub fn get(&self, id: &PluginId) -> Option<PluginEntry> {
        self.plugins.read().expect("plugin registry lock poisoned").get(id).cloned()
    }
    /// All enabled plugins.
    pub fn list_enabled(&self) -> Vec<PluginEntry> {
        self.plugins
            .read()
            .expect("plugin registry lock poisoned")
            .values()
            .filter(|e| e.status == PluginStatus::Enabled || e.status == PluginStatus::Degraded)
            .cloned()
            .collect()
    }
    /// All disabled plugins.
    pub fn list_disabled(&self) -> Vec<PluginEntry> {
        self.plugins
            .read()
            .expect("plugin registry lock poisoned")
            .values()
            .filter(|e| e.status == PluginStatus::Disabled)
            .cloned()
            .collect()
    }
    /// All plugins regardless of status.
    pub fn list_all(&self) -> Vec<PluginEntry> {
        self.plugins.read().expect("plugin registry lock poisoned").values().cloned().collect()
    }
    /// Check if a plugin is installed (present in registry, any status).
    pub fn is_installed(&self, id: &PluginId) -> bool {
        self.plugins.read().expect("plugin registry lock poisoned").contains_key(id)
    }
    /// Number of registered plugins.
    pub fn len(&self) -> usize {
        self.plugins.read().expect("plugin registry lock poisoned").len()
    }
    /// Returns `true` if no plugins are registered.
    pub fn is_empty(&self) -> bool {
        self.plugins.read().expect("plugin registry lock poisoned").is_empty()
    }

    fn validate_dependencies(&self, id: &PluginId) -> Result<(), PluginError> {
        fn visit(
            id: &PluginId,
            plugins: &HashMap<PluginId, PluginEntry>,
            stack: &mut Vec<PluginId>,
            visited: &mut std::collections::HashSet<PluginId>,
        ) -> Result<(), PluginError> {
            if let Some(index) = stack.iter().position(|candidate| candidate == id) {
                let mut cycle = stack[index..].to_vec();
                cycle.push(id.clone());
                return Err(PluginError::CircularDependency { cycle });
            }
            if !visited.insert(id.clone()) {
                return Ok(());
            }
            let entry = plugins.get(id).ok_or_else(|| PluginError::PluginNotFound {
                plugin_id: id.to_string(),
                marketplace: id.marketplace.clone(),
            })?;
            stack.push(id.clone());
            for dependency in &entry.plugin.manifest.dependencies {
                let dependency_id = dependency.resolve(&entry.plugin.id.marketplace);
                let dependency_entry = plugins.get(&dependency_id).ok_or_else(|| {
                    PluginError::DependencyUnsatisfied {
                        dependency: dependency_id.to_string(),
                        reason: DependencyReason::NotFound,
                    }
                })?;
                if !dependency.version.matches(&dependency_entry.plugin.manifest.version) {
                    return Err(PluginError::DependencyVersionConflict {
                        plugin: Box::new(dependency_id.clone()),
                        required: Box::new(dependency.version.clone()),
                        actual: Box::new(dependency_entry.plugin.manifest.version.clone()),
                        required_by: Box::new(id.clone()),
                    });
                }
                visit(&dependency_id, plugins, stack, visited)?;
                if !matches!(
                    dependency_entry.status,
                    PluginStatus::Enabled | PluginStatus::Degraded
                ) {
                    return Err(PluginError::DependencyUnsatisfied {
                        dependency: dependency_id.to_string(),
                        reason: DependencyReason::NotEnabled,
                    });
                }
            }
            stack.pop();
            Ok(())
        }

        let plugins = self.plugins.read().expect("plugin registry lock poisoned");
        visit(id, &plugins, &mut Vec::new(), &mut std::collections::HashSet::new())
    }
}
