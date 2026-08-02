use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;
use telos_agent::{MarketplaceRegistry, PluginId, PluginRegistry};

use crate::agent_host::{DesktopSettingsOverrides, resolve_desktop_settings};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPluginInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub source_status: String,
    pub status: String,
    pub errors: Vec<String>,
    pub config_schema: Value,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopMarketplacePlugin {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub installed: bool,
}

pub fn list_plugins(cwd: Option<PathBuf>) -> Result<Vec<DesktopPluginInfo>, String> {
    let root = plugins_root(cwd)?;
    let registry = load_plugin_registry(&root)?;
    let marketplaces = load_marketplaces(&root)?;
    let mut plugins = registry
        .list_all()
        .into_iter()
        .map(|entry| {
            let config = registry
                .resolved_config(&entry.plugin.id)
                .map(|config| serde_json::to_value(config.redacted_values()).unwrap_or(Value::Null))
                .unwrap_or(Value::Null);
            DesktopPluginInfo {
                id: entry.plugin.id.to_string(),
                name: entry.plugin.manifest.name,
                description: entry.plugin.manifest.description,
                version: entry.plugin.manifest.version.to_string(),
                source_status: marketplaces.source_status(&entry.plugin.id).as_str().to_string(),
                status: format!("{:?}", entry.status).to_lowercase(),
                errors: entry.load_errors.into_iter().map(|error| error.to_string()).collect(),
                config_schema: serde_json::to_value(entry.plugin.manifest.user_config)
                    .unwrap_or(Value::Null),
                config,
            }
        })
        .collect::<Vec<_>>();
    plugins.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(plugins)
}

pub fn set_plugin_enabled(cwd: Option<PathBuf>, id: &str, enabled: bool) -> Result<(), String> {
    let registry = plugin_registry(cwd)?;
    let id = parse_plugin_id(id)?;
    if enabled {
        registry.enable(&id).map_err(|error| error.to_string())?;
    } else {
        registry.disable(&id).map_err(|error| error.to_string())?;
    }
    registry.save_state().map_err(|error| error.to_string())
}

pub fn set_plugin_config(
    cwd: Option<PathBuf>,
    id: &str,
    values: HashMap<String, Value>,
) -> Result<(), String> {
    let registry = plugin_registry(cwd)?;
    registry
        .set_config(&parse_plugin_id(id)?, values)
        .map_err(|error| error.to_string())
}

pub fn clear_plugin_config(cwd: Option<PathBuf>, id: &str) -> Result<(), String> {
    let registry = plugin_registry(cwd)?;
    registry.clear_config(&parse_plugin_id(id)?).map_err(|error| error.to_string())
}

pub fn list_marketplace_plugins(
    cwd: Option<PathBuf>,
) -> Result<Vec<DesktopMarketplacePlugin>, String> {
    let root = plugins_root(cwd)?;
    let registry = load_plugin_registry(&root)?;
    let mut marketplaces = MarketplaceRegistry::new(&root);
    marketplaces.load().map_err(|error| error.to_string())?;
    let mut plugins = Vec::new();
    for marketplace in marketplaces.names() {
        if let Some(catalog) = marketplaces.get(marketplace) {
            plugins.extend(catalog.plugins.iter().map(|entry| {
                let id = PluginId { name: entry.name.clone(), marketplace: marketplace.clone() };
                DesktopMarketplacePlugin {
                    id: id.to_string(),
                    name: entry.name.clone(),
                    description: entry.description.clone(),
                    version: entry.version.to_string(),
                    installed: registry.is_installed(&id),
                }
            }));
        }
    }
    plugins.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(plugins)
}

pub fn install_plugin(cwd: Option<PathBuf>, id: &str) -> Result<(), String> {
    let root = plugins_root(cwd)?;
    let registry = load_plugin_registry(&root)?;
    let mut marketplaces = load_marketplaces(&root)?;
    let id = parse_plugin_id(id)?;
    registry
        .refresh_marketplace(&mut marketplaces, &id.marketplace)
        .map_err(|error| error.to_string())?;
    registry.install(&marketplaces, &id).map_err(|error| error.to_string())
}

pub fn upgrade_plugin(cwd: Option<PathBuf>, id: &str) -> Result<(), String> {
    let root = plugins_root(cwd)?;
    let registry = load_plugin_registry(&root)?;
    let mut marketplaces = load_marketplaces(&root)?;
    let id = parse_plugin_id(id)?;
    registry
        .refresh_marketplace(&mut marketplaces, &id.marketplace)
        .map_err(|error| error.to_string())?;
    registry.upgrade(&marketplaces, &id).map_err(|error| error.to_string())
}

pub fn uninstall_plugin(cwd: Option<PathBuf>, id: &str) -> Result<(), String> {
    let root = plugins_root(cwd)?;
    load_plugin_registry(&root)?
        .uninstall(&parse_plugin_id(id)?)
        .map_err(|error| error.to_string())
}

fn plugin_registry(cwd: Option<PathBuf>) -> Result<PluginRegistry, String> {
    let root = plugins_root(cwd)?;
    load_plugin_registry(&root)
}

fn plugins_root(cwd: Option<PathBuf>) -> Result<PathBuf, String> {
    let settings = resolve_desktop_settings(&DesktopSettingsOverrides {
        cwd,
        ..DesktopSettingsOverrides::default()
    })?;
    Ok(settings.project_root_or_cwd.join(".telos").join("plugins"))
}

fn load_plugin_registry(root: &std::path::Path) -> Result<PluginRegistry, String> {
    std::fs::create_dir_all(root.join("installed")).map_err(|error| error.to_string())?;
    let registry = PluginRegistry::new(root);
    registry.discover_installed().map_err(|error| error.to_string())?;
    registry.load_state().map_err(|error| error.to_string())?;
    registry.load_config().map_err(|error| error.to_string())?;
    Ok(registry)
}

fn load_marketplaces(root: &std::path::Path) -> Result<MarketplaceRegistry, String> {
    let mut marketplaces = MarketplaceRegistry::new(root);
    marketplaces.load().map_err(|error| error.to_string())?;
    Ok(marketplaces)
}

fn parse_plugin_id(value: &str) -> Result<PluginId, String> {
    PluginId::parse(value).ok_or_else(|| "plugin id must use name@marketplace".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use telos_agent::{Marketplace, MarketplaceEntry, MarketplaceSource, PluginSource};

    #[test]
    fn lists_toggles_and_configures_project_plugins() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(".git")).unwrap();
        let plugin = project
            .path()
            .join(".telos/plugins/installed/configurable@test");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            plugin.join("plugin.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "manifestVersion": 2,
                "name": "configurable",
                "version": "1.0.0",
                "userConfig": {
                    "token": {
                        "type": "string",
                        "title": "Token",
                        "description": "Secret token",
                        "required": true,
                        "sensitive": true
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        set_plugin_config(
            Some(project.path().into()),
            "configurable@test",
            HashMap::from([("token".into(), Value::String("secret".into()))]),
        )
        .unwrap();
        set_plugin_enabled(Some(project.path().into()), "configurable@test", true).unwrap();
        let plugins = list_plugins(Some(project.path().into())).unwrap();

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].status, "enabled");
        assert_eq!(plugins[0].config["token"], "[REDACTED]");

        clear_plugin_config(Some(project.path().into()), "configurable@test").unwrap();
        assert!(list_plugins(Some(project.path().into())).unwrap()[0].config.is_null());
    }

    #[test]
    fn installs_upgrades_and_uninstalls_from_registered_marketplace() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(".git")).unwrap();
        let source = project.path().join("plugin-source");
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
        let marketplace_dir = project.path().join("marketplace");
        std::fs::create_dir_all(&marketplace_dir).unwrap();
        std::fs::write(
            marketplace_dir.join("marketplace.json"),
            serde_json::to_vec_pretty(&Marketplace {
                name: "local".into(),
                owner: None,
                plugins: vec![MarketplaceEntry {
                    name: "managed".into(),
                    description: None,
                    version: "1.0.0".parse().unwrap(),
                    source: PluginSource::Local { path: source },
                    category: None,
                    tags: Vec::new(),
                    strict: true,
                    manifest_override: None,
                }],
                allow_cross_marketplace_deps_on: None,
            })
            .unwrap(),
        )
        .unwrap();
        let root = project.path().join(".telos/plugins");
        let mut marketplaces = MarketplaceRegistry::new(&root);
        marketplaces
            .add_named(MarketplaceSource::Local { path: marketplace_dir }, Some("local".into()))
            .unwrap();
        marketplaces.save().unwrap();

        install_plugin(Some(project.path().into()), "managed@local").unwrap();
        assert!(list_marketplace_plugins(Some(project.path().into()))
            .unwrap()
            .into_iter()
            .any(|plugin| plugin.id == "managed@local" && plugin.installed));
        upgrade_plugin(Some(project.path().into()), "managed@local").unwrap();
        uninstall_plugin(Some(project.path().into()), "managed@local").unwrap();
        assert!(list_plugins(Some(project.path().into())).unwrap().is_empty());
    }
}
