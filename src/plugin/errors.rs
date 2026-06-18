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
    #[error("plugin '{id}' is degraded — {loaded}/{total} components loaded")]
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
