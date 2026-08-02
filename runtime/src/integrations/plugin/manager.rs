//! Single-writer facade for plugin and marketplace management.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde_json::Value;

use crate::integrations::plugin::{
    MarketplaceRegistry, MarketplaceSource, PluginError, PluginId, PluginRegistry,
};

/// Owns the installed-plugin and marketplace views while holding the project
/// plugin lock. Management callers should open one manager per operation.
pub struct PluginManager {
    registry: PluginRegistry,
    marketplaces: MarketplaceRegistry,
    _lock: File,
}

impl PluginManager {
    /// Open the project plugin store and acquire its cross-process writer lock.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, PluginError> {
        let root = root.into();
        std::fs::create_dir_all(root.join("installed"))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(".lock"))?;
        lock.lock_exclusive()?;
        reject_legacy_state(&root)?;

        let registry = PluginRegistry::new(&root);
        registry.discover_installed()?;
        registry.load_state()?;
        registry.load_config()?;
        let mut marketplaces = MarketplaceRegistry::new(&root);
        marketplaces.load()?;

        Ok(Self { registry, marketplaces, _lock: lock })
    }

    pub fn registry(&self) -> &PluginRegistry {
        &self.registry
    }

    pub fn marketplaces(&self) -> &MarketplaceRegistry {
        &self.marketplaces
    }

    pub fn enable(&self, id: &PluginId) -> Result<(), PluginError> {
        self.registry.enable(id)?;
        self.registry.save_state()
    }

    pub fn disable(&self, id: &PluginId) -> Result<(), PluginError> {
        self.registry.disable(id)?;
        self.registry.save_state()
    }

    pub fn set_config(
        &self,
        id: &PluginId,
        values: HashMap<String, Value>,
    ) -> Result<(), PluginError> {
        self.registry.set_config(id, values)
    }

    pub fn clear_config(&self, id: &PluginId) -> Result<(), PluginError> {
        self.registry.clear_config(id)
    }

    pub fn install(&mut self, id: &PluginId) -> Result<(), PluginError> {
        self.registry.refresh_marketplace(&mut self.marketplaces, &id.marketplace)?;
        self.registry.install(&self.marketplaces, id)
    }

    pub fn install_local(
        &mut self,
        plugin_dir: impl AsRef<Path>,
        marketplace: impl Into<String>,
    ) -> Result<PluginId, PluginError> {
        let plugin_dir = std::fs::canonicalize(plugin_dir)?;
        if !plugin_dir.is_dir() {
            return Err(PluginError::Other(format!(
                "{} is not a plugin directory",
                plugin_dir.display()
            )));
        }
        let manifest = crate::integrations::plugin::registry::read_manifest_from_dir(&plugin_dir)?;
        let marketplace = marketplace.into();
        if !crate::integrations::plugin::is_valid_id_part(&marketplace) {
            return Err(PluginError::Other(format!("invalid marketplace name `{marketplace}`")));
        }
        let catalog_dir =
            self.marketplaces.upsert_local_plugin(&marketplace, &plugin_dir, &manifest)?;
        if self.marketplaces.get(&marketplace).is_some() {
            self.refresh_marketplace(&marketplace)?;
        } else {
            self.add_marketplace(
                MarketplaceSource::Local { path: catalog_dir },
                Some(marketplace.clone()),
            )?;
        }
        let id = PluginId { name: manifest.name, marketplace };
        self.install(&id)?;
        Ok(id)
    }

    pub fn upgrade(&mut self, id: &PluginId) -> Result<(), PluginError> {
        self.registry.refresh_marketplace(&mut self.marketplaces, &id.marketplace)?;
        self.registry.upgrade(&self.marketplaces, id)
    }

    pub fn uninstall(&self, id: &PluginId) -> Result<(), PluginError> {
        self.registry.uninstall(id)
    }

    pub fn refresh_marketplace(
        &mut self,
        name: &str,
    ) -> Result<crate::integrations::plugin::MarketplaceRefreshReport, PluginError> {
        self.registry.refresh_marketplace(&mut self.marketplaces, name)
    }

    pub fn add_marketplace(
        &mut self,
        source: MarketplaceSource,
        name: Option<String>,
    ) -> Result<String, PluginError> {
        let name = self.marketplaces.add_named(source, name)?;
        self.marketplaces.save()?;
        Ok(name)
    }

    pub fn remove_marketplace(&mut self, name: &str) -> Result<(), PluginError> {
        self.registry.remove_marketplace(&mut self.marketplaces, name)
    }

    pub fn into_registry(self) -> PluginRegistry {
        self.registry
    }
}

fn reject_legacy_state(root: &Path) -> Result<(), PluginError> {
    let state = root.join("state.json");
    if state.exists() {
        return Ok(());
    }
    let legacy = ["plugin_state.json", "plugin_config.json", "known_marketplaces.json"]
        .into_iter()
        .map(|name| root.join(name))
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    if legacy.is_empty() {
        return Ok(());
    }
    Err(PluginError::Other(format!(
        "legacy plugin v2 state is not migrated automatically; back up and remove: {}",
        legacy.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn manager_serializes_writers_for_the_same_root() {
        let temp = tempfile::tempdir().unwrap();
        let first = PluginManager::open(temp.path()).unwrap();
        let root = temp.path().to_path_buf();
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let second = PluginManager::open(root).unwrap();
            sender.send(()).unwrap();
            drop(second);
        });

        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
        drop(first);
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn legacy_state_is_reported_without_deleting_it() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("plugin_state.json");
        std::fs::write(&legacy, "{}").unwrap();

        let error = PluginManager::open(temp.path()).err().unwrap();

        assert!(error.to_string().contains("not migrated automatically"));
        assert!(legacy.exists());
    }

    #[test]
    fn install_local_owns_catalog_creation_and_installation() {
        let temp = tempfile::tempdir().unwrap();
        let plugin_dir = temp.path().join("source");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::json!({
                "manifestVersion": 3,
                "name": "local-plugin",
                "version": "1.0.0"
            })
            .to_string(),
        )
        .unwrap();

        let root = temp.path().join("plugins");
        let mut manager = PluginManager::open(&root).unwrap();
        let id = manager.install_local(&plugin_dir, "local").unwrap();

        assert_eq!(id.to_string(), "local-plugin@local");
        assert!(manager.registry().is_installed(&id));
        assert!(root.join("local-marketplaces/local/marketplace.json").is_file());
    }

    #[test]
    fn install_local_rejects_missing_manifest_before_catalog_write() {
        let temp = tempfile::tempdir().unwrap();
        let plugin_dir = temp.path().join("source");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let root = temp.path().join("plugins");
        let mut manager = PluginManager::open(&root).unwrap();

        assert!(manager.install_local(&plugin_dir, "local").is_err());
        assert!(!root.join("local-marketplaces").exists());
    }
}
