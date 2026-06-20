use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Configuration loaded from toml files (user-level ~/.config/telos/config.toml
/// and project-level .telos.toml).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FileConfig {
    pub agent: Option<AgentSection>,
    pub approval: Option<ApprovalSection>,
    pub diagnostics: Option<DiagnosticsSection>,
    pub tui: Option<TuiSection>,
    pub env: Option<HashMap<String, String>>,
    /// Whether to auto-approve tool calls by default.
    pub auto_mode: Option<bool>,
}

/// Terminal UI configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TuiSection {
    pub density: Option<TuiDensity>,
}

/// Terminal UI density preset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TuiDensity {
    Compact,
    #[default]
    Default,
    Spacious,
}

/// Local diagnostics and optional external reporting configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DiagnosticsSection {
    pub enabled: Option<bool>,
    pub retention_days: Option<u64>,
    pub github: Option<DiagnosticsGithubSection>,
}

/// GitHub issue reporter configuration for sanitized diagnostics summaries.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DiagnosticsGithubSection {
    pub enabled: Option<bool>,
    pub repository: Option<String>,
    pub interval_hours: Option<u64>,
    pub min_occurrences: Option<usize>,
}

/// Model routing configuration from [agent.models] TOML section.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelsSection {
    pub thinking: Option<String>,
    pub fast: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentSection {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub max_iterations: Option<usize>,
    pub models: Option<ModelsSection>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApprovalSection {
    pub default_policy: Option<String>,
    pub policies: Option<HashMap<String, String>>,
}
