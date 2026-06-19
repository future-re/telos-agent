//! CodeQL configuration — runtime feature gating and parameterization.

use std::path::PathBuf;

/// Runtime configuration for the CodeQL integration.
///
/// CodeQL is gated at runtime (not at compile time via Cargo features), following
/// the project convention of using optional struct fields rather than `#[cfg]`.
///
/// When `enabled` is `true` and the `codeql` CLI is available on PATH, the
/// [`CodeQLTool`](super::tool::CodeQLTool) is registered and background startup
/// analysis runs via [`CodeQLRuntime`](super::CodeQLRuntime).
#[derive(Debug, Clone)]
pub struct CodeqlConfig {
    /// Whether CodeQL analysis is enabled.  Default: `false`.
    pub enabled: bool,
    /// Query packs or suites to run during startup analysis.
    /// Examples: `["codeql/rust-queries", "codeql/suite/security-extended"]`
    pub query_packs: Vec<String>,
    /// Maximum number of findings reported per query.  Default: 20.
    pub max_results: usize,
    /// Timeout for a single `codeql database analyze` invocation (seconds).  Default: 300.
    pub query_timeout_secs: u64,
    /// Timeout for `codeql database create` (seconds).  Default: 600.
    pub db_create_timeout_secs: u64,
    /// Project language hint.  When `None` the language is auto-detected from
    /// project files (`Cargo.toml`, `package.json`, etc.).
    pub language: Option<String>,
    /// Explicit path to a pre-built CodeQL database.
    /// When `None`, a database is auto-managed under `.codeql/dbs/`.
    pub database_path: Option<PathBuf>,
}

impl Default for CodeqlConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            query_packs: vec![
                "codeql/rust-queries".into(),
                "codeql/suite/security-extended".into(),
            ],
            max_results: 20,
            query_timeout_secs: 300,
            db_create_timeout_secs: 600,
            language: None,
            database_path: None,
        }
    }
}
