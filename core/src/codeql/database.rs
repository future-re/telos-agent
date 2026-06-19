//! CodeQL database lifecycle — detection, creation, and incremental updates.
//!
//! Manages a `.codeql/dbs/` directory inside the project root, with a
//! `.db-meta.json` sidecar for tracking when sources have changed so we
//! can skip (or shorten) database rebuilds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::codeql::config::CodeqlConfig;

/// Metadata stored alongside a CodeQL database so we can decide whether a
/// full rebuild or an incremental update is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbMeta {
    version: u32,
    language: String,
    last_built_ts: u64,
    git_head: Option<String>,
    file_mtimes: HashMap<String, u64>,
}

impl DbMeta {
    fn path(db_dir: &Path) -> PathBuf {
        db_dir.join(".db-meta.json")
    }

    fn read(db_dir: &Path) -> Option<Self> {
        let data = std::fs::read_to_string(Self::path(db_dir)).ok()?;
        serde_json::from_str(&data).ok()
    }

    fn write(&self, db_dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(Self::path(db_dir), json)
    }
}

/// Manages a CodeQL database for a project.
#[derive(Debug)]
pub struct CodeqlDatabase {
    /// Path to the database directory on disk.
    pub path: PathBuf,
    /// Detected or configured language.
    pub language: String,
}

impl CodeqlDatabase {
    /// Resolve the database path from config or derive it from `project_root`.
    pub fn resolve_path(config: &CodeqlConfig, language: &str, project_root: &Path) -> PathBuf {
        config
            .database_path
            .clone()
            .unwrap_or_else(|| project_root.join(".codeql").join("dbs").join(language))
    }

    /// Check whether the `codeql` CLI is available on `PATH`.
    pub async fn cli_available() -> bool {
        Command::new("codeql")
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Detect the most likely CodeQL-supported language for the project.
    ///
    /// Returns `None` when no well-known project file is found.
    pub fn detect_language(project_root: &Path) -> Option<String> {
        let indicators: &[(&str, &str)] = &[
            ("Cargo.toml", "rust"),
            ("package.json", "javascript"),
            ("tsconfig.json", "javascript"),
            ("go.mod", "go"),
            ("pom.xml", "java"),
            ("build.gradle", "java"),
            ("build.gradle.kts", "java"),
            ("requirements.txt", "python"),
            ("setup.py", "python"),
            ("pyproject.toml", "python"),
            ("CMakeLists.txt", "cpp"),
            ("Gemfile", "ruby"),
            ("composer.json", "php"),
            ("Package.swift", "swift"),
            ("*.csproj", "csharp"),
        ];

        for (file, lang) in indicators {
            if file.starts_with('*') {
                let pattern = file.trim_start_matches('*');
                if glob_file(project_root, pattern) {
                    return Some((*lang).into());
                }
            } else if project_root.join(file).exists() {
                return Some((*lang).into());
            }
        }
        None
    }

    /// Check whether the database needs to be rebuilt by comparing the stored
    /// metadata against the current state of the project.
    pub async fn needs_rebuild(db_path: &Path, project_root: &Path) -> bool {
        let meta = match DbMeta::read(db_path) {
            Some(m) => m,
            None => return true, // No metadata → definitely needs rebuild.
        };

        // Check git HEAD if we have one stored.
        if let Some(ref stored_head) = meta.git_head {
            if let Some(current_head) = current_git_head(project_root) {
                if current_head != *stored_head {
                    return true;
                }
            }
        }

        // Check file mtimes for key project files.
        for (rel_path, &stored_mtime) in &meta.file_mtimes {
            let abs = project_root.join(rel_path);
            if let Ok(meta) = tokio::fs::metadata(&abs).await {
                if let Some(mtime_secs) = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                {
                    if mtime_secs > stored_mtime {
                        return true;
                    }
                }
            } else {
                // File deleted or moved — rebuild to be safe.
                return true;
            }
        }

        false
    }

    /// Build a fresh CodeQL database.
    pub async fn create(
        db_path: &Path,
        language: &str,
        project_root: &Path,
        timeout_dur: Duration,
    ) -> Result<(), String> {
        let lang_arg = format!("--language={language}");
        let source_root_arg = format!("--source-root={}", project_root.display());
        let db_path_str = db_path.to_string_lossy().to_string();
        let output = tokio::time::timeout(timeout_dur, {
            Command::new("codeql")
                .args([
                    "database",
                    "create",
                    &db_path_str,
                    &lang_arg,
                    &source_root_arg,
                    "--overwrite",
                ])
                .output()
        })
        .await
        .map_err(|_| "timed out creating CodeQL database".to_string())?
        .map_err(|e| format!("failed to run codeql database create: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("codeql database create failed: {stderr}"));
        }

        // Write metadata for future incremental checks.
        if let Err(e) = write_meta(db_path, language, project_root).await {
            tracing::warn!("failed to write CodeQL db meta: {e}");
        }

        Ok(())
    }

    /// Update an existing CodeQL database incrementally.
    pub async fn update(db_path: &Path, timeout_dur: Duration) -> Result<(), String> {
        let output = tokio::time::timeout(timeout_dur, {
            Command::new("codeql").args(["database", "update", &db_path.to_string_lossy()]).output()
        })
        .await
        .map_err(|_| "timed out updating CodeQL database".to_string())?
        .map_err(|e| format!("failed to run codeql database update: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("codeql database update failed: {stderr}"));
        }

        Ok(())
    }

    /// Ensure a database exists — create it if missing, update if stale.
    ///
    /// This is the main entry point for the runtime.  Returns the action taken
    /// (`"skipped"`, `"created"`, `"updated"`).
    pub async fn ensure(
        config: &CodeqlConfig,
        language: &str,
        project_root: &Path,
    ) -> Result<(Self, String), String> {
        let db_path = Self::resolve_path(config, language, project_root);
        let db_exists =
            db_path.join("src.zip").exists() || db_path.join("codeql-database.yml").exists();

        let action = if !db_exists {
            Self::create(
                &db_path,
                language,
                project_root,
                Duration::from_secs(config.db_create_timeout_secs),
            )
            .await?;
            // Re-read the language from the database after creation since it may
            // be more precise than our heuristic.
            let actual_lang = language.to_string();
            "created"
        } else if Self::needs_rebuild(&db_path, project_root).await {
            Self::update(&db_path, Duration::from_secs(config.db_create_timeout_secs)).await?;
            "updated"
        } else {
            "skipped"
        };

        Ok((Self { path: db_path, language: language.to_string() }, action.to_string()))
    }
}

// ── helpers ──────────────────────────────────────────────────────────────

async fn write_meta(db_path: &Path, language: &str, project_root: &Path) -> std::io::Result<()> {
    let git_head = current_git_head(project_root);
    let mut file_mtimes = HashMap::new();

    let lang_files: &[&str] = match language {
        "rust" => &["Cargo.toml", "Cargo.lock", "src/"],
        "javascript" => &["package.json", "package-lock.json", "src/", "tsconfig.json"],
        "python" => &["requirements.txt", "setup.py", "pyproject.toml"],
        _ => &[],
    };

    for rel in lang_files {
        let abs = project_root.join(rel);
        if rel.ends_with('/') {
            // Directory — collect files up to depth 2.
            collect_mtimes(&abs, &mut file_mtimes, project_root, 2);
        } else if let Ok(meta) = tokio::fs::metadata(&abs).await {
            if let Ok(mtime) = meta.modified().and_then(|t| t.duration_since(std::time::UNIX_EPOCH))
            {
                file_mtimes.insert(rel.to_string(), mtime.as_secs());
            }
        }
    }

    let meta = DbMeta {
        version: 1,
        language: language.into(),
        last_built_ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        git_head,
        file_mtimes,
    };

    meta.write(db_path)
}

fn collect_mtimes(dir: &Path, map: &mut HashMap<String, u64>, root: &Path, max_depth: u32) {
    if max_depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else { continue };
        let rel_str = rel.to_string_lossy().to_string();
        if rel_str.contains("target/") || rel_str.contains("node_modules/") {
            continue;
        }
        if path.is_dir() {
            collect_mtimes(&path, map, root, max_depth - 1);
        } else if let Ok(file_meta) = std::fs::metadata(&path) {
            if let Some(mtime_secs) = file_meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
            {
                map.insert(rel_str, mtime_secs);
            }
        }
    }
}

fn current_git_head(project_root: &Path) -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn glob_file(root: &Path, suffix: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|e| e.file_name().to_string_lossy().ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rust_via_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        let lang = CodeqlDatabase::detect_language(dir.path());
        assert_eq!(lang, Some("rust".into()));
    }

    #[test]
    fn detect_javascript_via_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let lang = CodeqlDatabase::detect_language(dir.path());
        assert_eq!(lang, Some("javascript".into()));
    }

    #[test]
    fn detect_python_via_pyproject_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[tool]\n").unwrap();
        let lang = CodeqlDatabase::detect_language(dir.path());
        assert_eq!(lang, Some("python".into()));
    }

    #[test]
    fn no_language_detected_for_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let lang = CodeqlDatabase::detect_language(dir.path());
        assert_eq!(lang, None);
    }

    #[test]
    fn db_path_resolution_defaults_to_codeql_dir() {
        let config = CodeqlConfig::default();
        let root = Path::new("/fake/project");
        let path = CodeqlDatabase::resolve_path(&config, "rust", root);
        assert_eq!(path, Path::new("/fake/project/.codeql/dbs/rust"));
    }

    #[test]
    fn db_path_resolution_uses_explicit_path() {
        let mut config = CodeqlConfig::default();
        config.database_path = Some(PathBuf::from("/opt/codeql-dbs/my-project"));
        let root = Path::new("/fake/project");
        let path = CodeqlDatabase::resolve_path(&config, "rust", root);
        assert_eq!(path, Path::new("/opt/codeql-dbs/my-project"));
    }
}
