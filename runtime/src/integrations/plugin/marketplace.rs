//! Marketplace registry — manages marketplace sources and their plugin catalogs.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::integrations::plugin::PluginError;
use crate::integrations::plugin::manifest::{MarketplaceEntry, PluginAuthor};
use crate::integrations::plugin::sources::MarketplaceSource;

/// A curated collection of plugins fetched from a marketplace source.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Marketplace {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<PluginAuthor>,
    pub plugins: Vec<MarketplaceEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_cross_marketplace_deps_on: Option<Vec<String>>,
}

/// Cached marketplace data stored on disk.
#[derive(Debug, Clone)]
struct CachedMarketplace {
    source: MarketplaceSource,
    manifest: Marketplace,
    /// Where the marketplace is cached on disk.
    install_location: PathBuf,
    /// When the marketplace was last refreshed (unix timestamp seconds).
    last_updated: u64,
}

/// Manages marketplace sources and provides plugin discovery across them.
pub struct MarketplaceRegistry {
    marketplaces: HashMap<String, CachedMarketplace>,
    cache_root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginSourceStatus {
    Available,
    RemovedFromMarketplace,
    MarketplaceMissing,
}

impl PluginSourceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::RemovedFromMarketplace => "removed-from-marketplace",
            Self::MarketplaceMissing => "marketplace-missing",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MarketplaceRefreshReport {
    pub orphaned: Vec<crate::integrations::plugin::PluginId>,
}

pub(crate) struct MarketplaceSnapshot {
    name: String,
    cached: CachedMarketplace,
    backup: Option<PathBuf>,
}

impl MarketplaceRegistry {
    /// Create a new marketplace registry. Cache goes under `cache_root/marketplaces/`.
    pub fn new(cache_root: impl Into<PathBuf>) -> Self {
        Self { marketplaces: HashMap::new(), cache_root: cache_root.into() }
    }

    /// Add a marketplace source. For local/inline sources, this is immediate.
    /// For remote sources (GitHub, git, URL, npm), fetching happens in
    /// `refresh()`.
    ///
    /// Returns the marketplace name.
    pub fn add(&mut self, source: MarketplaceSource) -> Result<String, PluginError> {
        self.add_named(source, None)
    }

    /// Add a marketplace with an optional stable local alias.
    pub fn add_named(
        &mut self,
        source: MarketplaceSource,
        requested_name: Option<String>,
    ) -> Result<String, PluginError> {
        let derived_name = match &source {
            MarketplaceSource::GitHub { repo, .. } => {
                // Derive name from repo: strip org, keep repo name
                repo.split('/').next_back().unwrap_or(repo).to_string()
            }
            MarketplaceSource::Git { url, .. } => {
                // Derive name from URL: last path segment without .git
                url.trim_end_matches('/')
                    .trim_end_matches(".git")
                    .split('/')
                    .next_back()
                    .unwrap_or("unknown")
                    .to_string()
            }
            MarketplaceSource::Url { url } => url
                .trim_end_matches('/')
                .split('/')
                .next_back()
                .unwrap_or("unknown")
                .split(['?', '#'])
                .next()
                .unwrap_or("unknown")
                .to_string(),
            MarketplaceSource::Npm { package } => {
                package.trim_start_matches('@').replace(['/', '@'], "-")
            }
            MarketplaceSource::Local { path } => {
                path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string()
            }
            MarketplaceSource::Inline { name, .. } => name.clone(),
        };
        let name = requested_name.unwrap_or(derived_name);

        if !crate::integrations::plugin::is_valid_id_part(&name) {
            return Err(PluginError::Other(format!(
                "invalid marketplace name `{name}`; use ASCII letters, digits, dots, hyphens, or underscores"
            )));
        }
        if self.marketplaces.contains_key(&name) {
            return Err(PluginError::Other(format!(
                "marketplace `{name}` is already registered; refresh or remove it first"
            )));
        }

        let install_location = self.cache_root.join("marketplaces").join(&name);

        // For local and inline sources, load immediately
        let (manifest, last_updated) = match &source {
            MarketplaceSource::Local { path } => {
                let manifest = Self::load_manifest_from_dir(path)?;
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                (manifest, timestamp)
            }
            MarketplaceSource::Inline { name, plugins } => {
                let manifest = Marketplace {
                    name: name.clone(),
                    owner: None,
                    plugins: plugins.clone(),
                    allow_cross_marketplace_deps_on: None,
                };
                (manifest, 0)
            }
            _ => {
                // Remote sources: create a placeholder; refresh() fills it in
                let manifest = Marketplace {
                    name: name.clone(),
                    owner: None,
                    plugins: Vec::new(),
                    allow_cross_marketplace_deps_on: None,
                };
                (manifest, 0)
            }
        };
        validate_marketplace(&manifest)?;

        self.marketplaces.insert(
            name.clone(),
            CachedMarketplace { source, manifest, install_location, last_updated },
        );

        Ok(name)
    }

    /// Remove a marketplace and any Telos-owned cached data.
    pub(crate) fn remove_unchecked(&mut self, name: &str) -> Result<(), PluginError> {
        if !crate::integrations::plugin::is_valid_id_part(name) {
            return Err(PluginError::Other(format!("invalid marketplace name `{name}`")));
        }
        if !self.marketplaces.contains_key(name) {
            return Err(PluginError::MarketplaceNotFound {
                marketplace: name.to_string(),
                available: self.marketplaces.keys().cloned().collect(),
            });
        }
        let owned_cache = self.cache_root.join("marketplaces").join(name);
        if owned_cache.exists() {
            std::fs::remove_dir_all(&owned_cache)?;
        }
        self.marketplaces.remove(name);
        Ok(())
    }

    /// Get the marketplace manifest by name.
    pub fn get(&self, name: &str) -> Option<&Marketplace> {
        self.marketplaces.get(name).map(|c| &c.manifest)
    }

    /// List all registered marketplace names.
    pub fn names(&self) -> Vec<&String> {
        self.marketplaces.keys().collect()
    }

    /// Search across all marketplaces for plugins matching `query`
    /// (case-insensitive substring match on name and description).
    pub fn search(&self, query: &str) -> Vec<(&Marketplace, &MarketplaceEntry)> {
        let query = query.to_lowercase();
        let mut results = Vec::new();
        for cached in self.marketplaces.values() {
            for entry in &cached.manifest.plugins {
                if entry.name.to_lowercase().contains(&query)
                    || entry.description.as_ref().is_some_and(|d| d.to_lowercase().contains(&query))
                {
                    results.push((&cached.manifest, entry));
                }
            }
        }
        results
    }

    /// List all available plugins across all marketplaces.
    pub fn list_all(&self) -> Vec<(&Marketplace, &MarketplaceEntry)> {
        let mut results = Vec::new();
        for cached in self.marketplaces.values() {
            for entry in &cached.manifest.plugins {
                results.push((&cached.manifest, entry));
            }
        }
        results
    }

    pub fn plugin_entry(
        &self,
        id: &crate::integrations::plugin::PluginId,
    ) -> Option<MarketplaceEntry> {
        self.marketplaces
            .get(&id.marketplace)
            .and_then(|cached| cached.manifest.plugins.iter().find(|entry| entry.name == id.name))
            .cloned()
    }

    pub(crate) fn source_base(&self, marketplace: &str) -> Option<PathBuf> {
        let cached = self.marketplaces.get(marketplace)?;
        match &cached.source {
            MarketplaceSource::Local { path } => Some(path.clone()),
            _ => Some(cached.install_location.clone()),
        }
    }

    pub(crate) fn allows_dependency(&self, from: &str, dependency: &str) -> bool {
        from == dependency
            || self
                .marketplaces
                .get(from)
                .and_then(|cached| cached.manifest.allow_cross_marketplace_deps_on.as_ref())
                .is_some_and(|allowed| allowed.iter().any(|name| name == dependency))
    }

    pub fn source_status(&self, id: &crate::integrations::plugin::PluginId) -> PluginSourceStatus {
        match self.marketplaces.get(&id.marketplace) {
            None => PluginSourceStatus::MarketplaceMissing,
            Some(cached) if cached.manifest.plugins.iter().any(|entry| entry.name == id.name) => {
                PluginSourceStatus::Available
            }
            Some(_) => PluginSourceStatus::RemovedFromMarketplace,
        }
    }

    /// Save known marketplaces metadata to `known_marketplaces.json`.
    pub fn save(&self) -> Result<(), PluginError> {
        let path = self.cache_root.join("known_marketplaces.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data: HashMap<String, serde_json::Value> = self
            .marketplaces
            .iter()
            .map(|(name, cached)| {
                let entry = serde_json::json!({
                    "source": cached.source,
                    "manifest": cached.manifest,
                    "installLocation": cached.install_location,
                    "lastUpdated": cached.last_updated,
                });
                (name.clone(), entry)
            })
            .collect();
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "marketplaces": data,
        }))?;
        atomic_write_file(&path, json.as_bytes())?;
        Ok(())
    }

    /// Load known marketplaces from `known_marketplaces.json`.
    pub fn load(&mut self) -> Result<(), PluginError> {
        let path = self.cache_root.join("known_marketplaces.json");
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(&path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;
        if let Some(marketplaces) = value.get("marketplaces").and_then(|v| v.as_object()) {
            for (name, entry) in marketplaces {
                if self.marketplaces.contains_key(name) {
                    continue; // already registered, skip
                }
                let source: MarketplaceSource = match serde_json::from_value(
                    entry.get("source").cloned().unwrap_or_default(),
                ) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let install_location = entry
                    .get("installLocation")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.cache_root.join("marketplaces").join(name));
                let last_updated = entry.get("lastUpdated").and_then(|v| v.as_u64()).unwrap_or(0);

                // Prefer the last catalog that completed coordinated refresh. This prevents
                // local source edits from bypassing deletion and dependency reconciliation
                // when a process restarts.
                let persisted_manifest = entry
                    .get("manifest")
                    .cloned()
                    .and_then(|value| serde_json::from_value::<Marketplace>(value).ok())
                    .filter(|manifest| validate_marketplace(manifest).is_ok());
                let manifest = persisted_manifest.unwrap_or_else(|| match &source {
                    MarketplaceSource::Local { path } => Self::load_manifest_from_dir(path)
                        .unwrap_or_else(|_| Marketplace {
                            name: name.clone(),
                            owner: None,
                            plugins: Vec::new(),
                            allow_cross_marketplace_deps_on: None,
                        }),
                    MarketplaceSource::Inline { name: inline_name, plugins } => Marketplace {
                        name: inline_name.clone(),
                        owner: None,
                        plugins: plugins.clone(),
                        allow_cross_marketplace_deps_on: None,
                    },
                    _ => Self::load_manifest_from_dir(&install_location).unwrap_or_else(|_| {
                        Marketplace {
                            name: name.clone(),
                            owner: None,
                            plugins: Vec::new(),
                            allow_cross_marketplace_deps_on: None,
                        }
                    }),
                });

                self.marketplaces.insert(
                    name.clone(),
                    CachedMarketplace { source, manifest, install_location, last_updated },
                );
            }
        }
        Ok(())
    }

    /// Load a marketplace manifest from a directory containing marketplace.json.
    fn load_manifest_from_dir(dir: &Path) -> Result<Marketplace, PluginError> {
        let manifest_path = dir.join("marketplace.json");
        let content =
            std::fs::read_to_string(&manifest_path).map_err(|e| PluginError::ManifestParse {
                path: manifest_path.clone(),
                reason: format!("failed to read: {e}"),
            })?;
        let manifest: Marketplace = serde_json::from_str(&content).map_err(|e| {
            PluginError::ManifestParse { path: manifest_path, reason: format!("invalid JSON: {e}") }
        })?;
        validate_marketplace(&manifest)?;
        Ok(manifest)
    }

    /// Refresh a marketplace from its declared source and atomically replace its cache.
    pub(crate) fn refresh_unchecked(&mut self, name: &str) -> Result<(), PluginError> {
        let cached =
            self.marketplaces.get(name).ok_or_else(|| PluginError::MarketplaceNotFound {
                marketplace: name.to_string(),
                available: self.marketplaces.keys().cloned().collect(),
            })?;

        let source = cached.source.clone();
        match &source {
            MarketplaceSource::Local { path } => {
                let manifest = Self::load_manifest_from_dir(path)?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                if let Some(cached) = self.marketplaces.get_mut(name) {
                    cached.manifest = manifest;
                    cached.last_updated = now;
                }
                Ok(())
            }
            MarketplaceSource::Inline { .. } => {
                // Inline marketplaces are immutable; nothing to refresh.
                Ok(())
            }
            MarketplaceSource::GitHub { repo, ref_, path } => {
                let url = format!("https://github.com/{repo}.git");
                self.refresh_git(name, &url, ref_.as_deref(), path.as_deref())
            }
            MarketplaceSource::Git { url, ref_, path } => {
                self.refresh_git(name, url, ref_.as_deref(), path.as_deref())
            }
            MarketplaceSource::Url { url } => self.refresh_url(name, url),
            MarketplaceSource::Npm { package } => self.refresh_npm(name, package),
        }
    }

    pub(crate) fn snapshot(&self, name: &str) -> Result<MarketplaceSnapshot, PluginError> {
        let cached = self.marketplaces.get(name).cloned().ok_or_else(|| {
            PluginError::MarketplaceNotFound {
                marketplace: name.to_string(),
                available: self.marketplaces.keys().cloned().collect(),
            }
        })?;
        let backup = if cached.install_location.exists()
            && !matches!(
                cached.source,
                MarketplaceSource::Local { .. } | MarketplaceSource::Inline { .. }
            ) {
            let backup = self.staging_dir(&format!("{name}-snapshot"));
            reset_directory(&backup)?;
            copy_directory(&cached.install_location, &backup)?;
            Some(backup)
        } else {
            None
        };
        Ok(MarketplaceSnapshot { name: name.to_string(), cached, backup })
    }

    pub(crate) fn restore_snapshot(
        &mut self,
        mut snapshot: MarketplaceSnapshot,
    ) -> Result<(), PluginError> {
        if let Some(backup) = snapshot.backup.take() {
            replace_directory(&backup, &snapshot.cached.install_location)?;
        }
        self.marketplaces.insert(snapshot.name, snapshot.cached);
        Ok(())
    }

    pub(crate) fn finish_snapshot(&self, snapshot: MarketplaceSnapshot) {
        if let Some(backup) = snapshot.backup
            && backup.exists()
            && let Err(error) = std::fs::remove_dir_all(&backup)
        {
            tracing::warn!(path = %backup.display(), %error, "failed to clean marketplace snapshot");
        }
    }

    fn refresh_git(
        &mut self,
        name: &str,
        url: &str,
        reference: Option<&str>,
        subpath: Option<&str>,
    ) -> Result<(), PluginError> {
        let staging = self.staging_dir(name);
        reset_directory(&staging)?;
        let _cleanup = CleanupDirectory(staging.clone());
        let mut command = std::process::Command::new("git");
        command.args(["clone", "--depth", "1"]);
        if let Some(reference) = reference {
            command.args(["--branch", reference]);
        }
        command.arg(url).arg(&staging);
        isolate_marketplace_environment(&mut command);
        let (status, stderr) = run_command_bounded(&mut command).map_err(|error| {
            PluginError::GitCloneFailed { url: url.into(), reason: error.to_string() }
        })?;
        if !status.success() {
            return Err(PluginError::GitCloneFailed { url: url.into(), reason: stderr });
        }
        let root = if let Some(path) = subpath {
            let path = safe_relative_path(path)?;
            let root = std::fs::canonicalize(staging.join(path))?;
            let canonical_staging = std::fs::canonicalize(&staging)?;
            if !root.starts_with(&canonical_staging) {
                return Err(PluginError::Other("marketplace git subpath escapes checkout".into()));
            }
            root
        } else {
            staging.clone()
        };
        let manifest = Self::load_manifest_from_dir(&root)?;
        let result = self.commit_cache(name, &root, manifest);
        let _ = std::fs::remove_dir_all(staging);
        result
    }

    fn refresh_url(&mut self, name: &str, url: &str) -> Result<(), PluginError> {
        const MAX_MARKETPLACE_BYTES: u64 = 10 * 1024 * 1024;
        let response = reqwest::blocking::Client::new()
            .get(url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|error| PluginError::NetworkError {
                url: url.into(),
                detail: error.to_string(),
            })?;
        if response.content_length().is_some_and(|length| length > MAX_MARKETPLACE_BYTES) {
            return Err(PluginError::NetworkError {
                url: url.into(),
                detail: "marketplace manifest exceeds 10 MiB".into(),
            });
        }
        let mut bytes = Vec::new();
        response.take(MAX_MARKETPLACE_BYTES + 1).read_to_end(&mut bytes).map_err(|error| {
            PluginError::NetworkError { url: url.into(), detail: error.to_string() }
        })?;
        if bytes.len() as u64 > MAX_MARKETPLACE_BYTES {
            return Err(PluginError::NetworkError {
                url: url.into(),
                detail: "marketplace manifest exceeds 10 MiB".into(),
            });
        }
        let manifest: Marketplace =
            serde_json::from_slice(&bytes).map_err(|error| PluginError::ManifestParse {
                path: PathBuf::from(url),
                reason: format!("invalid JSON: {error}"),
            })?;
        validate_marketplace(&manifest)?;
        let staging = self.staging_dir(name);
        reset_directory(&staging)?;
        let _cleanup = CleanupDirectory(staging.clone());
        std::fs::write(staging.join("marketplace.json"), bytes)?;
        let result = self.commit_cache(name, &staging, manifest);
        let _ = std::fs::remove_dir_all(staging);
        result
    }

    fn refresh_npm(&mut self, name: &str, package: &str) -> Result<(), PluginError> {
        let staging = self.staging_dir(name);
        reset_directory(&staging)?;
        let _cleanup = CleanupDirectory(staging.clone());
        let mut command = std::process::Command::new("npm");
        command
            .args(["install", "--ignore-scripts", "--no-audit", "--no-fund", "--prefix"])
            .arg(&staging)
            .arg(package);
        isolate_marketplace_environment(&mut command);
        let (status, stderr) = run_command_bounded(&mut command).map_err(|error| {
            PluginError::NpmInstallFailed { package: package.into(), reason: error.to_string() }
        })?;
        if !status.success() {
            return Err(PluginError::NpmInstallFailed { package: package.into(), reason: stderr });
        }
        let root = staging.join("node_modules").join(package);
        let manifest = Self::load_manifest_from_dir(&root)?;
        let result = self.commit_cache(name, &root, manifest);
        let _ = std::fs::remove_dir_all(staging);
        result
    }

    fn staging_dir(&self, name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        self.cache_root.join(".staging").join(format!("{name}-{nonce}"))
    }

    fn commit_cache(
        &mut self,
        name: &str,
        source_root: &Path,
        manifest: Marketplace,
    ) -> Result<(), PluginError> {
        let target = self.cache_root.join("marketplaces").join(name);
        let prepared = self.staging_dir(&format!("{name}-prepared"));
        copy_directory(source_root, &prepared)?;
        replace_directory(&prepared, &target)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let available = self.marketplaces.keys().cloned().collect();
        let cached = self.marketplaces.get_mut(name).ok_or_else(|| {
            PluginError::MarketplaceNotFound { marketplace: name.into(), available }
        })?;
        cached.manifest = manifest;
        cached.install_location = target;
        cached.last_updated = now;
        Ok(())
    }
}

fn safe_relative_path(path: &str) -> Result<&Path, PluginError> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(PluginError::Other(format!("unsafe marketplace subpath `{}`", path.display())));
    }
    Ok(path)
}

fn validate_marketplace(manifest: &Marketplace) -> Result<(), PluginError> {
    let mut errors = Vec::new();
    if !crate::integrations::plugin::is_valid_id_part(&manifest.name) {
        errors.push(format!("invalid marketplace name `{}`", manifest.name));
    }
    let mut names = std::collections::HashSet::new();
    for entry in &manifest.plugins {
        if !crate::integrations::plugin::is_valid_id_part(&entry.name) {
            errors.push(format!("invalid plugin entry name `{}`", entry.name));
        }
        if !names.insert(&entry.name) {
            errors.push(format!("duplicate plugin entry `{}`", entry.name));
        }
    }
    if let Some(allowed) = &manifest.allow_cross_marketplace_deps_on {
        for marketplace in allowed {
            if !crate::integrations::plugin::is_valid_id_part(marketplace) {
                errors.push(format!("invalid allowed dependency marketplace `{marketplace}`"));
            }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(PluginError::ManifestValidation { errors }) }
}

fn isolate_marketplace_environment(command: &mut std::process::Command) {
    command.env_clear().envs(crate::config::platform_base_env());
}

fn atomic_write_file(path: &Path, contents: &[u8]) -> Result<(), PluginError> {
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, contents)?;
    #[cfg(windows)]
    {
        let backup = path.with_extension("json.backup");
        if backup.exists() {
            std::fs::remove_file(&backup)?;
        }
        if path.exists() {
            std::fs::rename(path, &backup)?;
        }
        if let Err(error) = std::fs::rename(&temporary, path) {
            if backup.exists() {
                let _ = std::fs::rename(&backup, path);
            }
            return Err(error.into());
        }
        if backup.exists() {
            let _ = std::fs::remove_file(backup);
        }
    }
    #[cfg(not(windows))]
    std::fs::rename(temporary, path)?;
    Ok(())
}

struct CleanupDirectory(PathBuf);

impl Drop for CleanupDirectory {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

pub(crate) fn reset_directory(path: &Path) -> Result<(), PluginError> {
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    std::fs::create_dir_all(path)?;
    Ok(())
}

pub(crate) fn run_command_bounded(
    command: &mut std::process::Command,
) -> std::io::Result<(std::process::ExitStatus, String)> {
    const MAX_STDERR_BYTES: usize = 1024 * 1024;
    const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
    command.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    let stderr_pipe = child.stderr.take();
    let stderr_reader = std::thread::spawn(move || -> std::io::Result<(Vec<u8>, bool)> {
        let mut stderr = Vec::new();
        let mut truncated = false;
        let Some(mut pipe) = stderr_pipe else {
            return Ok((stderr, truncated));
        };
        let mut chunk = [0_u8; 8192];
        loop {
            let read = pipe.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            let remaining = MAX_STDERR_BYTES.saturating_sub(stderr.len());
            let retained = remaining.min(read);
            stderr.extend_from_slice(&chunk[..retained]);
            truncated |= retained < read;
        }
        Ok((stderr, truncated))
    });
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= COMMAND_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_reader.join();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "plugin source command timed out after 300 seconds",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let (stderr, truncated) = stderr_reader
        .join()
        .map_err(|_| std::io::Error::other("plugin source stderr reader panicked"))??;
    let mut stderr = String::from_utf8_lossy(&stderr).trim().to_string();
    if truncated {
        stderr.push_str("… [truncated]");
    }
    Ok((status, stderr))
}

pub(crate) fn copy_directory(source: &Path, target: &Path) -> Result<(), PluginError> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let destination = target.join(entry.file_name());
        if file_type.is_symlink() {
            return Err(PluginError::Other(format!(
                "marketplace cache refuses symlink {}",
                entry.path().display()
            )));
        }
        if file_type.is_dir() {
            copy_directory(&entry.path(), &destination)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

pub(crate) fn replace_directory(source: &Path, target: &Path) -> Result<(), PluginError> {
    let backup = target.with_extension("backup");
    if backup.exists() {
        std::fs::remove_dir_all(&backup)?;
    }
    if target.exists() {
        std::fs::rename(target, &backup)?;
    }
    if let Err(error) = std::fs::rename(source, target) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, target);
        }
        return Err(error.into());
    }
    if backup.exists()
        && let Err(error) = std::fs::remove_dir_all(&backup)
    {
        tracing::warn!(path = %backup.display(), %error, "failed to clean marketplace backup");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_marketplace_dir(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let manifest = serde_json::json!({
            "name": name,
            "owner": {"name": "Test Org"},
            "plugins": [
                {
                    "name": "test-plugin",
                    "version": "1.0.0",
                    "description": "A test plugin",
                    "source": {"type": "local", "path": "./test-plugin"},
                    "category": "testing",
                    "tags": ["test"]
                },
                {
                    "name": "another-plugin",
                    "version": "1.0.0",
                    "description": "Another one",
                    "source": {"type": "github", "repo": "org/repo"}
                }
            ]
        });
        std::fs::write(
            dir.join("marketplace.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn add_local_marketplace() {
        let tmp = TempDir::new().unwrap();
        let mkt_dir = tmp.path().join("my-marketplace");
        make_marketplace_dir(&mkt_dir, "my-marketplace");

        let mut registry = MarketplaceRegistry::new(tmp.path());
        let name = registry.add(MarketplaceSource::Local { path: mkt_dir }).unwrap();
        assert_eq!(name, "my-marketplace");

        let mkt = registry.get("my-marketplace").unwrap();
        assert_eq!(mkt.plugins.len(), 2);
        assert_eq!(mkt.plugins[0].name, "test-plugin");
    }

    #[test]
    fn add_inline_marketplace() {
        let tmp = TempDir::new().unwrap();
        let mut registry = MarketplaceRegistry::new(tmp.path());
        let name = registry
            .add(MarketplaceSource::Inline {
                name: "inline-mkt".into(),
                plugins: vec![MarketplaceEntry {
                    name: "my-plugin".into(),
                    description: Some("desc".into()),
                    version: semver::Version::new(1, 0, 0),
                    source: crate::integrations::plugin::manifest::PluginSource::Local {
                        path: "/tmp/p".into(),
                    },
                    category: None,
                    tags: vec![],
                    strict: true,
                    manifest_override: None,
                }],
            })
            .unwrap();
        assert_eq!(name, "inline-mkt");
        assert_eq!(registry.list_all().len(), 1);
    }

    #[test]
    fn search_finds_matching_plugins() {
        let tmp = TempDir::new().unwrap();
        let mkt_dir = tmp.path().join("mkt");
        make_marketplace_dir(&mkt_dir, "mkt");

        let mut registry = MarketplaceRegistry::new(tmp.path());
        registry.add(MarketplaceSource::Local { path: mkt_dir }).unwrap();

        let results = registry.search("test");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "test-plugin");

        let results = registry.search("another");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "another-plugin");

        let results = registry.search("nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn remove_marketplace() {
        let tmp = TempDir::new().unwrap();
        let mut registry = MarketplaceRegistry::new(tmp.path());
        registry.add(MarketplaceSource::Inline { name: "test".into(), plugins: vec![] }).unwrap();
        let cached = tmp.path().join("marketplaces/test");
        std::fs::create_dir_all(&cached).unwrap();
        std::fs::write(cached.join("marketplace.json"), "{}").unwrap();
        assert!(registry.names().contains(&&"test".to_string()));
        registry.remove_unchecked("test").unwrap();
        assert!(!registry.names().contains(&&"test".to_string()));
        assert!(!cached.exists());
    }

    #[test]
    fn save_and_load_marketplaces() {
        let tmp = TempDir::new().unwrap();
        let mkt_dir = tmp.path().join("my-mkt");
        make_marketplace_dir(&mkt_dir, "my-mkt");

        let mut registry = MarketplaceRegistry::new(tmp.path().join("cache"));
        registry.add(MarketplaceSource::Local { path: mkt_dir.clone() }).unwrap();
        registry.save().unwrap();

        std::fs::write(
            mkt_dir.join("marketplace.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "name": "my-mkt",
                "plugins": []
            }))
            .unwrap(),
        )
        .unwrap();

        let mut registry2 = MarketplaceRegistry::new(tmp.path().join("cache"));
        registry2.load().unwrap();
        assert!(registry2.get("my-mkt").is_some());
        assert_eq!(registry2.list_all().len(), 2);
    }

    #[test]
    fn load_reuses_cached_remote_marketplace_without_refresh() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache");
        let mut registry = MarketplaceRegistry::new(&cache);
        let name = registry
            .add(MarketplaceSource::Url { url: "https://example.invalid/community.json".into() })
            .unwrap();
        let cached = cache.join("marketplaces").join(&name);
        std::fs::create_dir_all(&cached).unwrap();
        std::fs::write(
            cached.join("marketplace.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "name": name,
                "plugins": [{
                    "name": "cached-plugin",
                    "version": "1.0.0",
                    "source": {"type": "local", "path": "./cached-plugin"}
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        registry.save().unwrap();
        let known_path = cache.join("known_marketplaces.json");
        let mut known: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&known_path).unwrap()).unwrap();
        known["marketplaces"][&name].as_object_mut().unwrap().remove("manifest");
        std::fs::write(&known_path, serde_json::to_vec_pretty(&known).unwrap()).unwrap();

        let mut loaded = MarketplaceRegistry::new(cache);
        loaded.load().unwrap();

        assert_eq!(loaded.get(&name).unwrap().plugins[0].name, "cached-plugin");
    }
}
