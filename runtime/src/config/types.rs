use std::collections::HashMap;

use serde::{Deserialize, Serialize};
pub use telos_agent::DefaultShell;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FileConfig {
    pub agent: Option<AgentSection>,
    pub approval: Option<ApprovalSection>,
    pub billing: Option<BillingSection>,
    pub diagnostics: Option<DiagnosticsSection>,
    pub tui: Option<TuiSection>,
    pub env: Option<HashMap<String, String>>,
    pub auto_mode: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TuiSection {
    pub density: Option<TuiDensity>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TuiDensity {
    Compact,
    #[default]
    Default,
    Spacious,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DiagnosticsSection {
    pub enabled: Option<bool>,
    pub retention_days: Option<u64>,
    pub github: Option<DiagnosticsGithubSection>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BillingSection {
    pub models: Option<HashMap<String, BillingModelPricing>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BillingModelPricing {
    pub input_cache_hit_per_million: Option<f64>,
    pub input_cache_miss_per_million: Option<f64>,
    pub output_per_million: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DiagnosticsGithubSection {
    pub enabled: Option<bool>,
    pub repository: Option<String>,
    pub interval_hours: Option<u64>,
    pub min_occurrences: Option<usize>,
}

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
    pub default_shell: Option<DefaultShell>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApprovalSection {
    pub default_policy: Option<String>,
    pub policies: Option<HashMap<String, String>>,
}
