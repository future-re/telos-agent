//! Transactional coordination between installed plugins and marketplace metadata.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::integrations::plugin::{
    MarketplaceRefreshReport, MarketplaceRegistry, PluginError, PluginId, PluginStatus,
};

impl PluginRegistry {
    pub fn refresh_marketplace(
        &self,
        marketplaces: &mut MarketplaceRegistry,
        name: &str,
    ) -> Result<MarketplaceRefreshReport, PluginError> {
        let _operation = self.operation_lock.lock().expect("plugin operation lock poisoned");
        let snapshot = marketplaces.snapshot(name)?;
        let old_names = marketplaces
            .get(name)
            .map(|marketplace| {
                marketplace.plugins.iter().map(|entry| entry.name.clone()).collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        if let Err(error) = marketplaces.refresh_unchecked(name) {
            marketplaces.finish_snapshot(snapshot);
            return Err(error);
        }
        let refreshed = marketplaces.get(name).expect("refreshed marketplace remains registered");
        let new_names =
            refreshed.plugins.iter().map(|entry| entry.name.clone()).collect::<HashSet<_>>();
        let force_remove = refreshed.force_remove_deleted_plugins;
        let mut removed = self
            .list_all()
            .into_iter()
            .filter(|entry| {
                entry.plugin.id.marketplace == name
                    && old_names.contains(&entry.plugin.id.name)
                    && !new_names.contains(&entry.plugin.id.name)
            })
            .map(|entry| entry.plugin.id)
            .collect::<Vec<_>>();
        removed.sort_by_key(ToString::to_string);

        if !force_remove {
            if let Err(error) = marketplaces.save() {
                let _ = marketplaces.restore_snapshot(snapshot);
                return Err(error);
            }
            marketplaces.finish_snapshot(snapshot);
            return Ok(MarketplaceRefreshReport { removed: Vec::new(), orphaned: removed });
        }

        let removed_set = removed.iter().cloned().collect::<HashSet<_>>();
        let installed = self.list_all();
        let mut blocked = installed
            .iter()
            .filter(|entry| {
                removed_set.contains(&entry.plugin.id) && entry.status != PluginStatus::Disabled
            })
            .map(|entry| entry.plugin.id.clone())
            .collect::<Vec<_>>();
        for entry in &installed {
            if removed_set.contains(&entry.plugin.id) {
                continue;
            }
            for dependency in &entry.plugin.manifest.dependencies {
                let dependency = dependency.resolve(&entry.plugin.id.marketplace);
                if removed_set.contains(&dependency) {
                    blocked.push(dependency);
                }
            }
        }
        blocked.sort_by_key(ToString::to_string);
        blocked.dedup();
        if !blocked.is_empty() {
            let _ = marketplaces.restore_snapshot(snapshot);
            return Err(PluginError::MarketplaceRefreshBlocked { plugins: blocked });
        }

        if let Err(error) = marketplaces.save() {
            let _ = marketplaces.restore_snapshot(snapshot);
            return Err(error);
        }
        if let Err(error) = self.remove_plugins_transaction(&removed) {
            let _ = marketplaces.restore_snapshot(snapshot);
            let _ = marketplaces.save();
            return Err(error);
        }
        marketplaces.finish_snapshot(snapshot);
        Ok(MarketplaceRefreshReport { removed, orphaned: Vec::new() })
    }

    pub fn remove_marketplace(
        &self,
        marketplaces: &mut MarketplaceRegistry,
        name: &str,
    ) -> Result<(), PluginError> {
        let _operation = self.operation_lock.lock().expect("plugin operation lock poisoned");
        let mut installed = self
            .list_all()
            .into_iter()
            .filter(|entry| entry.plugin.id.marketplace == name)
            .map(|entry| entry.plugin.id)
            .collect::<Vec<_>>();
        installed.sort_by_key(ToString::to_string);
        if !installed.is_empty() {
            return Err(PluginError::MarketplaceInUse {
                marketplace: name.to_string(),
                plugins: installed,
            });
        }
        let snapshot = marketplaces.snapshot(name)?;
        if let Err(error) = marketplaces.remove_unchecked(name).and_then(|_| marketplaces.save()) {
            let _ = marketplaces.restore_snapshot(snapshot);
            let _ = marketplaces.save();
            return Err(error);
        }
        marketplaces.finish_snapshot(snapshot);
        Ok(())
    }

    fn remove_plugins_transaction(&self, ids: &[PluginId]) -> Result<(), PluginError> {
        if ids.is_empty() {
            return Ok(());
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let backup_root = self.plugins_root.join(".trash").join(format!("refresh-{nonce}"));
        std::fs::create_dir_all(&backup_root)?;
        let previous_config =
            self.config_store.read().expect("plugin config lock poisoned").clone();
        let mut removed = Vec::new();
        for id in ids {
            let Some(entry) = self.get(id) else {
                continue;
            };
            let backup = backup_root.join(id.to_string());
            if entry.plugin.path.exists()
                && let Err(error) = std::fs::rename(&entry.plugin.path, &backup)
            {
                self.restore_removed_plugins(&removed, previous_config.clone());
                return Err(error.into());
            }
            self.remove(id);
            removed.push((id.clone(), entry, backup));
        }

        let result = self.save_state().and_then(|_| {
            self.config_store.write().expect("plugin config lock poisoned").remove_many(ids)
        });
        if let Err(error) = result {
            self.restore_removed_plugins(&removed, previous_config);
            return Err(error);
        }
        if let Err(error) = std::fs::remove_dir_all(&backup_root) {
            tracing::warn!(path = %backup_root.display(), %error, "failed to clean refresh backup");
        }
        Ok(())
    }

    fn restore_removed_plugins(
        &self,
        removed: &[(PluginId, super::types::PluginEntry, PathBuf)],
        previous_config: crate::integrations::plugin::PluginConfigStore,
    ) {
        for (id, entry, backup) in removed.iter().rev() {
            if backup.exists() {
                let _ = std::fs::rename(backup, &entry.plugin.path);
            }
            self.remove(id);
            super::install::restore_entry(self, entry.clone());
        }
        *self.config_store.write().expect("plugin config lock poisoned") = previous_config;
        let _ = self.config_store.read().expect("plugin config lock poisoned").save();
        let _ = self.save_state();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::plugin::{MarketplaceSource, PluginSourceStatus};

    fn setup() -> (tempfile::TempDir, PathBuf, MarketplaceRegistry, PluginRegistry, PluginId) {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("plugin-source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("plugin.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "manifestVersion": 2,
                "name": "managed",
                "version": "1.0.0"
            }))
            .unwrap(),
        )
        .unwrap();
        let marketplace_dir = temp.path().join("marketplace");
        std::fs::create_dir_all(&marketplace_dir).unwrap();
        write_marketplace(&marketplace_dir, &source, false, true);
        let root = temp.path().join("plugins");
        let mut marketplaces = MarketplaceRegistry::new(&root);
        marketplaces
            .add_named(
                MarketplaceSource::Local { path: marketplace_dir.clone() },
                Some("local".into()),
            )
            .unwrap();
        marketplaces.save().unwrap();
        let registry = PluginRegistry::new(&root);
        let id = PluginId::parse("managed@local").unwrap();
        registry.install(&marketplaces, &id).unwrap();
        (temp, marketplace_dir, marketplaces, registry, id)
    }

    fn write_marketplace(
        dir: &std::path::Path,
        source: &std::path::Path,
        force: bool,
        include: bool,
    ) {
        let plugins = if include {
            vec![serde_json::json!({
                "name": "managed",
                "version": "1.0.0",
                "source": {"type": "local", "path": source}
            })]
        } else {
            Vec::new()
        };
        std::fs::write(
            dir.join("marketplace.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "name": "local",
                "plugins": plugins,
                "forceRemoveDeletedPlugins": force
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn refresh_retains_orphan_when_force_remove_is_false() {
        let (_temp, marketplace_dir, mut marketplaces, registry, id) = setup();
        let source = registry.get(&id).unwrap().plugin.source;
        let crate::integrations::plugin::PluginSource::Local { path: source } = source else {
            unreachable!()
        };
        write_marketplace(&marketplace_dir, &source, false, false);

        let report = registry.refresh_marketplace(&mut marketplaces, "local").unwrap();

        assert_eq!(report.orphaned, vec![id.clone()]);
        assert!(registry.is_installed(&id));
        assert_eq!(marketplaces.source_status(&id), PluginSourceStatus::RemovedFromMarketplace);
    }

    #[test]
    fn refresh_safely_removes_disabled_deleted_plugin_and_blocks_enabled_plugin() {
        let (_temp, marketplace_dir, mut marketplaces, registry, id) = setup();
        let source = match registry.get(&id).unwrap().plugin.source {
            crate::integrations::plugin::PluginSource::Local { path } => path,
            _ => unreachable!(),
        };
        write_marketplace(&marketplace_dir, &source, true, false);
        let report = registry.refresh_marketplace(&mut marketplaces, "local").unwrap();
        assert_eq!(report.removed, vec![id.clone()]);
        assert!(!registry.is_installed(&id));

        write_marketplace(&marketplace_dir, &source, false, true);
        registry.refresh_marketplace(&mut marketplaces, "local").unwrap();
        registry.install(&marketplaces, &id).unwrap();
        registry.enable(&id).unwrap();
        write_marketplace(&marketplace_dir, &source, true, false);
        let error = registry.refresh_marketplace(&mut marketplaces, "local").unwrap_err();
        assert!(matches!(error, PluginError::MarketplaceRefreshBlocked { .. }));
        assert!(
            marketplaces.get("local").unwrap().plugins.iter().any(|entry| entry.name == "managed")
        );
        assert!(registry.is_installed(&id));
    }

    #[test]
    fn remove_marketplace_is_blocked_until_all_plugins_are_uninstalled() {
        let (_temp, _marketplace_dir, mut marketplaces, registry, id) = setup();
        let error = registry.remove_marketplace(&mut marketplaces, "local").unwrap_err();
        assert!(matches!(error, PluginError::MarketplaceInUse { .. }));
        registry.uninstall(&id).unwrap();
        registry.remove_marketplace(&mut marketplaces, "local").unwrap();
        assert!(marketplaces.get("local").is_none());
    }
}
