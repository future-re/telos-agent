//! Marketplace registry — manages marketplace sources and their plugin catalogs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::plugin::PluginError;
use crate::plugin::manifest::{MarketplaceEntry, PluginAuthor};
use crate::plugin::sources::MarketplaceSource;

/// A curated collection of plugins fetched from a marketplace source.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marketplace {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<PluginAuthor>,
    pub plugins: Vec<MarketplaceEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_remove_deleted_plugins: Option<bool>,
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
        let name = match &source {
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
            MarketplaceSource::Url { url } => {
                url.trim_end_matches('/').split('/').next_back().unwrap_or("unknown").to_string()
            }
            MarketplaceSource::Npm { package } => package.replace('/', "-"),
            MarketplaceSource::Local { path } => {
                path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string()
            }
            MarketplaceSource::Inline { name, .. } => name.clone(),
        };

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
                    force_remove_deleted_plugins: None,
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
                    force_remove_deleted_plugins: None,
                    allow_cross_marketplace_deps_on: None,
                };
                (manifest, 0)
            }
        };

        self.marketplaces.insert(
            name.clone(),
            CachedMarketplace { source, manifest, install_location, last_updated },
        );

        Ok(name)
    }

    /// Remove a marketplace and its cached data.
    pub fn remove(&mut self, name: &str) -> Result<(), PluginError> {
        self.marketplaces.remove(name).ok_or_else(|| PluginError::MarketplaceNotFound {
            marketplace: name.to_string(),
            available: self.marketplaces.keys().cloned().collect(),
        })?;
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
        std::fs::write(&path, json)?;
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

                // For disk-backed sources, try to load the manifest
                let manifest = match &source {
                    MarketplaceSource::Local { path } => Self::load_manifest_from_dir(path)
                        .unwrap_or_else(|_| Marketplace {
                            name: name.clone(),
                            owner: None,
                            plugins: Vec::new(),
                            force_remove_deleted_plugins: None,
                            allow_cross_marketplace_deps_on: None,
                        }),
                    MarketplaceSource::Inline { name: inline_name, plugins } => Marketplace {
                        name: inline_name.clone(),
                        owner: None,
                        plugins: plugins.clone(),
                        force_remove_deleted_plugins: None,
                        allow_cross_marketplace_deps_on: None,
                    },
                    _ => Marketplace {
                        name: name.clone(),
                        owner: None,
                        plugins: Vec::new(),
                        force_remove_deleted_plugins: None,
                        allow_cross_marketplace_deps_on: None,
                    },
                };

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
        Ok(manifest)
    }
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
                    "description": "A test plugin",
                    "source": {"type": "local", "path": "./test-plugin"},
                    "category": "testing",
                    "tags": ["test"]
                },
                {
                    "name": "another-plugin",
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
                    version: None,
                    source: crate::plugin::manifest::PluginSource::Local { path: "/tmp/p".into() },
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
        assert!(registry.names().contains(&&"test".to_string()));
        registry.remove("test").unwrap();
        assert!(!registry.names().contains(&&"test".to_string()));
    }

    #[test]
    fn save_and_load_marketplaces() {
        let tmp = TempDir::new().unwrap();
        let mkt_dir = tmp.path().join("my-mkt");
        make_marketplace_dir(&mkt_dir, "my-mkt");

        let mut registry = MarketplaceRegistry::new(tmp.path().join("cache"));
        registry.add(MarketplaceSource::Local { path: mkt_dir }).unwrap();
        registry.save().unwrap();

        let mut registry2 = MarketplaceRegistry::new(tmp.path().join("cache"));
        registry2.load().unwrap();
        assert!(registry2.get("my-mkt").is_some());
        assert_eq!(registry2.list_all().len(), 2);
    }
}
