//! Coordination between installed plugins and side-effect-free marketplace refreshes.

use std::collections::HashSet;

use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::integrations::plugin::{MarketplaceRefreshReport, MarketplaceRegistry, PluginError};

impl PluginRegistry {
    pub fn refresh_marketplace(
        &self,
        marketplaces: &mut MarketplaceRegistry,
        name: &str,
    ) -> Result<MarketplaceRefreshReport, PluginError> {
        let _operation = self.operation_lock.lock().expect("plugin operation lock poisoned");
        let snapshot = marketplaces.snapshot(name)?;
        if let Err(error) = marketplaces.refresh_unchecked(name) {
            marketplaces.finish_snapshot(snapshot);
            return Err(error);
        }
        let refreshed = marketplaces.get(name).expect("refreshed marketplace remains registered");
        let new_names =
            refreshed.plugins.iter().map(|entry| entry.name.clone()).collect::<HashSet<_>>();
        let mut orphaned = self
            .list_all()
            .into_iter()
            .filter(|entry| {
                entry.plugin.id.marketplace == name && !new_names.contains(&entry.plugin.id.name)
            })
            .map(|entry| entry.plugin.id)
            .collect::<Vec<_>>();
        orphaned.sort_by_key(ToString::to_string);

        if let Err(error) = marketplaces.save() {
            if let Err(rollback) = marketplaces.restore_snapshot(snapshot) {
                return Err(PluginError::Other(format!(
                    "marketplace save failed: {error}; restoring the previous catalog also failed: {rollback}"
                )));
            }
            return Err(error);
        }
        marketplaces.finish_snapshot(snapshot);
        Ok(MarketplaceRefreshReport { orphaned })
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
            if let Err(rollback) =
                marketplaces.restore_snapshot(snapshot).and_then(|_| marketplaces.save())
            {
                return Err(PluginError::Other(format!(
                    "marketplace removal failed: {error}; restoring it also failed: {rollback}"
                )));
            }
            return Err(error);
        }
        marketplaces.finish_snapshot(snapshot);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::plugin::{MarketplaceSource, PluginId, PluginSourceStatus};
    use std::path::PathBuf;

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
        write_marketplace(&marketplace_dir, &source, true);
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

    fn write_marketplace(dir: &std::path::Path, source: &std::path::Path, include: bool) {
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
                "plugins": plugins
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn refresh_reports_retained_orphan() {
        let (_temp, marketplace_dir, mut marketplaces, registry, id) = setup();
        let source = registry.get(&id).unwrap().plugin.source;
        let crate::integrations::plugin::PluginSource::Local { path: source } = source else {
            unreachable!()
        };
        write_marketplace(&marketplace_dir, &source, false);

        let report = registry.refresh_marketplace(&mut marketplaces, "local").unwrap();

        assert_eq!(report.orphaned, vec![id.clone()]);
        assert!(registry.is_installed(&id));
        assert_eq!(marketplaces.source_status(&id), PluginSourceStatus::RemovedFromMarketplace);
    }

    #[test]
    fn refresh_never_removes_orphans() {
        let (_temp, marketplace_dir, mut marketplaces, registry, id) = setup();
        let source = match registry.get(&id).unwrap().plugin.source {
            crate::integrations::plugin::PluginSource::Local { path } => path,
            _ => unreachable!(),
        };
        registry.enable(&id).unwrap();
        write_marketplace(&marketplace_dir, &source, false);
        let report = registry.refresh_marketplace(&mut marketplaces, "local").unwrap();
        assert_eq!(report.orphaned, vec![id.clone()]);
        assert!(registry.is_installed(&id));
        assert!(marketplaces.get("local").unwrap().plugins.is_empty());
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
