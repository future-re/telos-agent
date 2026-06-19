//! CodeQL runtime — session-startup background analysis and registration glue.
//!
//! Follows the same pattern as [`crate::memory_runtime`]: provides a
//! `register_codeql()` function that registers the [`CodeQLTool`] on the tool
//! registry and appends a [`CodeqlSection`] to the prompt assembly, then
//! returns an optional [`CodeQLRuntime`] for background startup analysis.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use telos_agent::{
    CodeQLTool, CodeqlConfig, CodeqlDatabase, CodeqlSection, MemoryStore, PromptAssembly,
    ToolRegistry,
};

use crate::config::FileConfig;

/// Report produced after the startup analysis completes (or is skipped).
#[derive(Debug)]
pub struct StartupReport {
    /// Whether the `codeql` CLI was found on PATH.
    pub available: bool,
    /// Action taken on the database: `"created"`, `"updated"`, `"skipped"`, or `""`.
    pub database_action: String,
    /// Path to the database used.
    pub database_path: Option<PathBuf>,
    /// Number of findings discovered.
    pub findings_count: usize,
    /// Duration of the analysis in milliseconds.
    pub duration_ms: u64,
    /// Error message, if any step failed.
    pub error: Option<String>,
}

/// Background runtime that runs CodeQL startup analysis.
pub struct CodeQLRuntime {
    config: CodeqlConfig,
    store: Arc<Mutex<MemoryStore>>,
    project_root: PathBuf,
}

impl CodeQLRuntime {
    pub fn new(
        config: CodeqlConfig,
        store: Arc<Mutex<MemoryStore>>,
        project_root: PathBuf,
    ) -> Self {
        Self { config, store, project_root }
    }

    /// Run startup analysis against the project.
    ///
    /// This is expected to be called from a `tokio::spawn` so it doesn't block
    /// the session from starting.
    pub async fn run_startup_analysis(&self) -> StartupReport {
        let started = Instant::now();

        // 1. Check CLI availability.
        if !CodeqlDatabase::cli_available().await {
            return StartupReport {
                available: false,
                database_action: String::new(),
                database_path: None,
                findings_count: 0,
                duration_ms: started.elapsed().as_millis() as u64,
                error: Some("codeql CLI not found on PATH".into()),
            };
        }

        // 2. Detect language.
        let language = match self
            .config
            .language
            .clone()
            .or_else(|| CodeqlDatabase::detect_language(&self.project_root))
        {
            Some(lang) => lang,
            None => {
                return StartupReport {
                    available: true,
                    database_action: String::new(),
                    database_path: None,
                    findings_count: 0,
                    duration_ms: started.elapsed().as_millis() as u64,
                    error: Some("could not detect project language".into()),
                };
            }
        };

        // 3. Ensure database exists.
        let (database, db_action) =
            match CodeqlDatabase::ensure(&self.config, &language, &self.project_root).await {
                Ok((db, action)) => (db, action),
                Err(e) => {
                    return StartupReport {
                        available: true,
                        database_action: "failed".into(),
                        database_path: None,
                        findings_count: 0,
                        duration_ms: started.elapsed().as_millis() as u64,
                        error: Some(format!("database ensure failed: {e}")),
                    };
                }
            };

        // 4. Run queries.
        let mut total_findings = 0;

        for pack in &self.config.query_packs {
            let result_file = std::env::temp_dir()
                .join(format!("codeql_startup_{}.sarif", pack.replace(['/', '\\', ' '], "_")));

            let child = tokio::process::Command::new("codeql")
                .args([
                    "database",
                    "analyze",
                    &database.path.to_string_lossy(),
                    pack,
                    "--format=sarif-latest",
                    "--output",
                    &result_file.to_string_lossy(),
                ])
                .output();

            let _ = match tokio::time::timeout(
                std::time::Duration::from_secs(self.config.query_timeout_secs),
                child,
            )
            .await
            {
                Ok(Ok(o)) if o.status.success() => o,
                Ok(Ok(o)) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    tracing::warn!(pack = %pack, "codeql analyze failed: {stderr}");
                    let _ = std::fs::remove_file(&result_file);
                    continue;
                }
                Ok(Err(e)) => {
                    tracing::warn!(pack = %pack, "failed to run codeql: {e}");
                    let _ = std::fs::remove_file(&result_file);
                    continue;
                }
                Err(_) => {
                    tracing::warn!(pack = %pack, "codeql analyze timed out");
                    let _ = std::fs::remove_file(&result_file);
                    continue;
                }
            };

            let sarif_json = match std::fs::read_to_string(&result_file) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(pack = %pack, "failed to read SARIF output: {e}");
                    let _ = std::fs::remove_file(&result_file);
                    continue;
                }
            };
            let _ = std::fs::remove_file(&result_file);

            let findings =
                match telos_agent::SarifParser::parse(&sarif_json, self.config.max_results) {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::warn!(pack = %pack, "failed to parse SARIF: {e}");
                        continue;
                    }
                };

            total_findings += findings.len();
            let store = self.store.clone();
            let source = "codeql-startup".to_string();
            let findings_for_store = findings;
            let _store_count = tokio::task::spawn_blocking(move || {
                let mut store = store.lock().map_err(|e| {
                    tracing::warn!("memory store poisoned: {e}");
                    0
                });
                let Ok(store) = store.as_deref_mut() else {
                    return 0;
                };
                let mut count = 0;
                for finding in &findings_for_store {
                    let entry = finding.to_memory_entry(Some(&source));
                    if store.upsert(entry).is_ok() {
                        count += 1;
                    }
                }
                count
            })
            .await
            .unwrap_or(0);
        }

        // 5. Mark stale findings as deprecated.
        // Any codeql-* memory that wasn't refreshed this run is now stale.
        {
            let store = self.store.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let mut store = store.lock().ok()?;
                let entries = store.query(telos_agent::MemoryQuery {
                    tags: vec!["codeql".into(), "analyzed".into()],
                    ..Default::default()
                });
                for entry in entries {
                    if entry.source_session.as_deref() == Some("codeql-startup")
                        && entry.status == telos_agent::MemoryStatus::NeedsFix
                    {
                        // Keep NeedsFix entries that were refreshed — they'll be
                        // upserted with a fresh timestamp.  Entries with old
                        // timestamps were not seen this run.
                        let ts = telos_agent::unix_timestamp();
                        if entry.updated != ts && entry.created < ts {
                            let _ = store
                                .update_status(&entry.name, telos_agent::MemoryStatus::Deprecated);
                        }
                    }
                }
                Some(())
            })
            .await;
        }

        StartupReport {
            available: true,
            database_action: db_action,
            database_path: Some(database.path),
            findings_count: total_findings,
            duration_ms: started.elapsed().as_millis() as u64,
            error: None,
        }
    }
}

/// Register the CodeQL tool and prompt section with the agent infrastructure.
///
/// Returns `Some(CodeQLRuntime)` when CodeQL is enabled and configured, so the
/// caller can spawn the startup analysis.  Returns `None` when disabled.
pub fn register_codeql(
    tools: &mut ToolRegistry,
    assembly: &mut PromptAssembly,
    store: Arc<Mutex<MemoryStore>>,
    config: Option<CodeqlConfig>,
    project_root: PathBuf,
) -> Option<CodeQLRuntime> {
    let config = match config {
        Some(c) if c.enabled => c,
        _ => return None,
    };

    let config = Arc::new(config);
    tools.register(CodeQLTool::new(config.clone(), store.clone()));
    assembly.add(CodeqlSection::new(store.clone()));

    Some(CodeQLRuntime::new(Arc::unwrap_or_clone(config), store, project_root))
}

/// Build a [`CodeqlConfig`] from the optional codeql section in [`FileConfig`].
pub fn codeql_config_from_file(file_config: &FileConfig) -> Option<CodeqlConfig> {
    let section = file_config.codeql.as_ref()?;
    let enabled = section.enabled.unwrap_or(false);
    if !enabled {
        return None;
    }

    Some(CodeqlConfig {
        enabled: true,
        query_packs: section.query_packs.clone().unwrap_or_else(|| {
            vec!["codeql/rust-queries".into(), "codeql/suite/security-extended".into()]
        }),
        max_results: section.max_results.unwrap_or(20),
        query_timeout_secs: section.timeout_secs.unwrap_or(300),
        db_create_timeout_secs: section.timeout_secs.unwrap_or(600),
        language: section.language.clone(),
        database_path: section.database_path.as_ref().map(PathBuf::from),
    })
}
