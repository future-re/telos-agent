//! Plugin registry lifecycle and query methods.

use crate::integrations::plugin::registry::types::{LoadedPlugin, PluginEntry, PluginStatus};
use crate::integrations::plugin::{PluginError, PluginId};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct PluginRegistry {
    pub(crate) plugins: HashMap<PluginId, PluginEntry>,
    pub(crate) plugins_root: PathBuf,
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
    /// Register a loaded plugin without enabling it.
    ///
    /// If a plugin with the same ID already exists, it is replaced
    /// (the old plugin's state is lost).
    pub fn register(&mut self, plugin: LoadedPlugin) {
        let status = if plugin.enabled { PluginStatus::Enabled } else { PluginStatus::Disabled };
        self.plugins.insert(plugin.id.clone(), PluginEntry::new(plugin, status));
    }
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
}
