//! [`CodeQLTool`] — agent-callable tool that runs CodeQL queries and persists
//! findings into the [`MemoryStore`].
//!
//! Follows the same pattern as [`MemoryReadTool`] and other built-in tools:
//! implement [`Tool`], spawn blocking work via `tokio::task::spawn_blocking`,
//! and gate availability at runtime.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::codeql::config::CodeqlConfig;
use crate::codeql::database::CodeqlDatabase;
use crate::codeql::sarif::SarifParser;
use crate::error::AgentError;
use crate::memory::MemoryStore;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

/// Runs CodeQL queries against a project database and stores findings as
/// persistent memories.
pub struct CodeQLTool {
    config: Arc<CodeqlConfig>,
    store: Arc<Mutex<MemoryStore>>,
}

impl CodeQLTool {
    pub fn new(config: Arc<CodeqlConfig>, store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { config, store }
    }
}

#[async_trait]
impl Tool for CodeQLTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "CodeQL".into(),
            description: "Run a CodeQL security/quality query against the project and optionally store findings as memories. Scans for vulnerabilities, code quality issues, and anti-patterns. Requires the codeql CLI to be installed.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "CodeQL query specifier: a suite name ('security-extended', 'security-and-quality'), a pack name ('codeql/rust-queries'), or a path to a .ql file."
                    },
                    "database": {
                        "type": "string",
                        "description": "Optional path to a CodeQL database. Defaults to the auto-managed database under .codeql/dbs/."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of findings to return. Default: 20, max: 200.",
                        "default": 20,
                        "minimum": 1,
                        "maximum": 200
                    },
                    "severity": {
                        "type": "string",
                        "enum": ["error", "warning", "all"],
                        "description": "Minimum severity to report. Default: warning.",
                        "default": "warning"
                    },
                    "store": {
                        "type": "boolean",
                        "description": "Whether to persist findings as memory entries. Default: true.",
                        "default": true
                    }
                },
                "required": ["query"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["codeql"]
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        // CodeQL is read-only analysis; no permission gate needed.
        Ok(PermissionDecision::Allow)
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        // Multiple concurrent CodeQL invocations on the same database will race.
        false
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let query = arguments
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::Validation("missing `query`".into()))?
            .to_string();

        let database_arg = arguments.get("database").and_then(|v| v.as_str()).map(String::from);

        let max_results = arguments
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|v| (v as usize).clamp(1, 200))
            .unwrap_or(20);

        let min_severity = arguments.get("severity").and_then(|v| v.as_str()).unwrap_or("warning");

        let do_store = arguments.get("store").and_then(|v| v.as_bool()).unwrap_or(true);

        // 1. Verify CLI is available.
        if !CodeqlDatabase::cli_available().await {
            return Ok(ToolOutput::json(json!({
                "error": "CodeQL CLI (`codeql`) not found on PATH. Install it from https://github.com/github/codeql-cli-binaries/releases",
                "results": [],
                "count": 0
            })));
        }

        // 2. Resolve database path.
        let db_path = match database_arg {
            Some(p) => PathBuf::from(p),
            None => {
                // Auto-detect language and resolve.
                let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let language = CodeqlDatabase::detect_language(&project_root);
                match language {
                    Some(lang) => CodeqlDatabase::resolve_path(&self.config, &lang, &project_root),
                    None => {
                        return Ok(ToolOutput::json(json!({
                            "error": "Could not auto-detect project language. Provide an explicit `database` path or set `language` in the codeql config.",
                            "results": [],
                            "count": 0
                        })));
                    }
                }
            }
        };

        if !db_path.exists() {
            return Ok(ToolOutput::json(json!({
                "error": format!("CodeQL database not found at {}. Build one first with: codeql database create {} --language=<lang> --source-root=.", db_path.display(), db_path.display()),
                "results": [],
                "count": 0
            })));
        }

        // 3. Run the query.
        let result_file =
            std::env::temp_dir().join(format!("codeql_result_{}.sarif", rand_like_id()));

        let child = Command::new("codeql")
            .args([
                "database",
                "analyze",
                &db_path.to_string_lossy(),
                &query,
                "--format=sarif-latest",
                "--output",
                &result_file.to_string_lossy(),
            ])
            .output();

        let output =
            match tokio::time::timeout(Duration::from_secs(self.config.query_timeout_secs), child)
                .await
            {
                Ok(Ok(o)) if o.status.success() => o,
                Ok(Ok(o)) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    let _ = std::fs::remove_file(&result_file);
                    return Ok(ToolOutput::json(json!({
                        "error": format!("codeql analyze failed: {stderr}"),
                        "query": query,
                        "results": [],
                        "count": 0
                    })));
                }
                Ok(Err(e)) => {
                    let _ = std::fs::remove_file(&result_file);
                    return Ok(ToolOutput::json(json!({
                        "error": format!("failed to run codeql: {e}"),
                        "query": query,
                        "results": [],
                        "count": 0
                    })));
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&result_file);
                    return Ok(ToolOutput::json(json!({
                        "error": "codeql analyze timed out",
                        "query": query,
                        "results": [],
                        "count": 0
                    })));
                }
            };

        // 4. Parse SARIF output.
        let sarif_json = std::fs::read_to_string(&result_file).unwrap_or_default();
        let _ = std::fs::remove_file(&result_file);

        let mut findings = match SarifParser::parse(&sarif_json, max_results) {
            Ok(f) => f,
            Err(e) => {
                return Ok(ToolOutput::json(json!({
                    "error": format!("failed to parse SARIF output: {e}"),
                    "query": query,
                    "results": [],
                    "count": 0
                })));
            }
        };

        // 5. Filter by severity.
        findings.retain(|f| match min_severity {
            "error" => f.severity == "error",
            "warning" => f.severity != "note",
            _ => true, // "all" — keep everything
        });

        // 6. Optionally store as memories.
        let stored_count = if do_store {
            let store = self.store.clone();
            let source_session = Some("codeql-tool".to_string());
            let findings_for_store = findings.clone();
            tokio::task::spawn_blocking(move || {
                let mut store = store.lock().map_err(|e| AgentError::ToolExecution {
                    tool: "CodeQL".into(),
                    message: format!("memory store poisoned: {e}"),
                })?;
                for finding in &findings_for_store {
                    let entry = finding.to_memory_entry(source_session.as_deref());
                    if let Err(e) = store.upsert(entry) {
                        tracing::warn!(rule_id = %finding.rule_id, "failed to store CodeQL finding: {e}");
                    }
                }
                Ok::<usize, AgentError>(findings_for_store.len())
            })
            .await
            .map_err(|e| AgentError::ToolExecution {
                tool: "CodeQL".into(),
                message: format!("memory task panicked: {e}"),
            })?
            .unwrap_or(0)
        } else {
            0
        };

        // 7. Return summary.
        let count = findings.len();
        let results: Vec<Value> = findings
            .iter()
            .map(|f| {
                json!({
                    "rule_id": f.rule_id,
                    "severity": f.severity,
                    "message": f.message,
                    "file": f.file_path,
                    "line": f.start_line,
                    "column": f.start_column
                })
            })
            .collect();

        Ok(ToolOutput::json(json!({
            "query": query,
            "database": db_path.to_string_lossy(),
            "count": count,
            "results": results,
            "stored_as_memories": stored_count
        })))
    }
}

/// Simple pseudo-random hex id for temp file names.  Not cryptographically
/// secure — collisions don't matter here because we clean up immediately.
fn rand_like_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:016x}")
}
