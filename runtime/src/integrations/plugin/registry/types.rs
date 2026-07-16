//! Types for the plugin registry.

use crate::integrations::plugin::manifest::PluginManifest;
use crate::integrations::plugin::{PluginError, PluginId, PluginSource};
use std::path::PathBuf;

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

#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub plugin: LoadedPlugin,
    pub status: PluginStatus,
    /// Errors from the last load attempt (empty if successful).
    pub load_errors: Vec<PluginError>,
}

impl PluginEntry {
    pub(crate) fn new(plugin: LoadedPlugin, status: PluginStatus) -> Self {
        Self { plugin, status, load_errors: Vec::new() }
    }
}
