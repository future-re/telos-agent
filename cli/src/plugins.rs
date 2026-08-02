use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::cli::{PluginCommand, SharedOptions};
use telos_agent::{
    Marketplace, MarketplaceEntry, MarketplaceRegistry, MarketplaceSource, PluginId, PluginManager,
    PluginManifest, PluginSource,
};

pub async fn run(command: PluginCommand, options: &SharedOptions) -> Result<()> {
    let cwd = options.cwd.clone().unwrap_or(std::env::current_dir()?);
    let project_root = crate::find_project_root(&cwd).unwrap_or(cwd);
    let plugins_root = project_root.join(".telos").join("plugins");
    let mut manager = PluginManager::open(&plugins_root)?;

    match command {
        PluginCommand::List => {
            let mut entries = manager.registry().list_all();
            entries.sort_by_key(|entry| entry.plugin.id.to_string());
            for entry in entries {
                println!(
                    "{}\t{:?}\t{}\t{}",
                    entry.plugin.id,
                    entry.status,
                    entry.plugin.manifest.version,
                    manager.marketplaces().source_status(&entry.plugin.id).as_str()
                );
                for error in entry.load_errors {
                    println!("  error: {error}");
                }
            }
        }
        PluginCommand::Inspect { id } => {
            let id = parse_id(&id)?;
            let entry = manager
                .registry()
                .get(&id)
                .ok_or_else(|| anyhow!("plugin `{id}` is not installed"))?;
            let config = manager.registry().resolved_config(&id);
            println!("id: {}", entry.plugin.id);
            println!("status: {:?}", entry.status);
            println!("version: {}", entry.plugin.manifest.version);
            println!("path: {}", entry.plugin.path.display());
            println!("source: {}", manager.marketplaces().source_status(&id).as_str());
            match config {
                Ok(config) => {
                    println!("config: {}", serde_json::to_string_pretty(&config.redacted_values())?)
                }
                Err(error) => println!("config: {error}"),
            }
        }
        PluginCommand::Enable { id } => {
            let id = parse_id(&id)?;
            manager.enable(&id)?;
            println!("enabled {id}");
        }
        PluginCommand::Disable { id } => {
            let id = parse_id(&id)?;
            manager.disable(&id)?;
            println!("disabled {id}");
        }
        PluginCommand::Install { id } => {
            let id = parse_id(&id)?;
            manager.install(&id)?;
            println!("installed {id}");
        }
        PluginCommand::InstallLocal { path, marketplace } => {
            let path = canonical_plugin_dir(&path)?;
            let manifest = read_manifest(&path)?;
            let id = PluginId { name: manifest.name.clone(), marketplace: marketplace.clone() };
            let catalog_dir = manager.root().join("local-marketplaces").join(&marketplace);
            std::fs::create_dir_all(&catalog_dir)?;
            let catalog_path = catalog_dir.join("marketplace.json");
            let mut catalog = if catalog_path.is_file() {
                serde_json::from_slice::<Marketplace>(&std::fs::read(&catalog_path)?)?
            } else {
                Marketplace { name: marketplace.clone(), owner: None, plugins: Vec::new() }
            };
            let local_entry = MarketplaceEntry {
                name: manifest.name,
                description: manifest.description,
                version: manifest.version,
                source: PluginSource::Local { path },
                category: None,
                tags: Vec::new(),
            };
            catalog.plugins.retain(|entry| entry.name != local_entry.name);
            catalog.plugins.push(local_entry);
            std::fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog)?)?;
            if manager.marketplaces().get(&marketplace).is_some() {
                manager.refresh_marketplace(&marketplace)?;
            } else {
                manager.add_marketplace(
                    MarketplaceSource::Local { path: catalog_dir },
                    Some(marketplace.clone()),
                )?;
            }
            manager.install(&id)?;
            println!("installed {id}");
        }
        PluginCommand::Upgrade { id } => {
            let id = parse_id(&id)?;
            manager.upgrade(&id)?;
            println!("upgraded {id}");
        }
        PluginCommand::Uninstall { id } => {
            let id = parse_id(&id)?;
            manager.uninstall(&id)?;
            println!("uninstalled {id}");
        }
        PluginCommand::Config { id, json } => {
            let id = parse_id(&id)?;
            let values: HashMap<String, serde_json::Value> =
                serde_json::from_str(&json).context("--json must be a JSON object")?;
            manager.set_config(&id, values)?;
            println!("configured {id}");
        }
        PluginCommand::ClearConfig { id } => {
            let id = parse_id(&id)?;
            manager.clear_config(&id)?;
            println!("cleared configuration for {id}");
        }
        PluginCommand::MarketplaceAddLocal { path, name } => {
            let name = manager.add_marketplace(
                MarketplaceSource::Local { path: std::fs::canonicalize(path)? },
                name,
            )?;
            println!("added marketplace {name}");
        }
        PluginCommand::MarketplaceAddGithub { repo, ref_, path, name } => {
            let name =
                manager.add_marketplace(MarketplaceSource::GitHub { repo, ref_, path }, name)?;
            manager.refresh_marketplace(&name)?;
            println!("added marketplace {name}");
        }
        PluginCommand::MarketplaceRefresh { name } => {
            let report = manager.refresh_marketplace(&name)?;
            for id in report.orphaned {
                println!("retained orphaned plugin {id}");
            }
            println!("refreshed marketplace {name}");
        }
        PluginCommand::MarketplaceRemove { name } => {
            manager.remove_marketplace(&name)?;
            println!("removed marketplace {name}");
        }
        PluginCommand::MarketplaceSearch { query } => {
            let query = query.to_lowercase();
            let mut matches = marketplace_entries(manager.marketplaces(), None)?
                .into_iter()
                .filter(|(_, entry)| {
                    entry.name.to_lowercase().contains(&query)
                        || entry
                            .description
                            .as_deref()
                            .is_some_and(|description| description.to_lowercase().contains(&query))
                        || entry.tags.iter().any(|tag| tag.to_lowercase().contains(&query))
                })
                .collect::<Vec<_>>();
            matches.sort_by(|left, right| left.0.cmp(&right.0));
            for (id, entry) in matches {
                println!("{id}\t{}\t{}", entry.version, entry.description.as_deref().unwrap_or(""));
            }
        }
        PluginCommand::MarketplaceListPlugins { name } => {
            let mut entries = marketplace_entries(manager.marketplaces(), name.as_deref())?;
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            for (id, entry) in entries {
                println!("{id}\t{}\t{}", entry.version, entry.description.as_deref().unwrap_or(""));
            }
        }
        PluginCommand::MarketplaceList => {
            let mut names = manager.marketplaces().names().into_iter().cloned().collect::<Vec<_>>();
            names.sort();
            for name in names {
                let count = manager
                    .marketplaces()
                    .get(&name)
                    .map(|marketplace| marketplace.plugins.len())
                    .unwrap_or(0);
                println!("{name}\t{count} plugins");
            }
        }
    }
    Ok(())
}

fn marketplace_entries<'a>(
    marketplaces: &'a MarketplaceRegistry,
    selected: Option<&str>,
) -> Result<Vec<(String, &'a MarketplaceEntry)>> {
    if let Some(selected) = selected
        && marketplaces.get(selected).is_none()
    {
        return Err(anyhow!("marketplace `{selected}` is not registered"));
    }
    let mut entries = Vec::new();
    for marketplace in marketplaces.names() {
        if selected.is_some_and(|selected| selected != marketplace) {
            continue;
        }
        if let Some(catalog) = marketplaces.get(marketplace) {
            entries.extend(
                catalog
                    .plugins
                    .iter()
                    .map(|entry| (format!("{}@{marketplace}", entry.name), entry)),
            );
        }
    }
    Ok(entries)
}

fn parse_id(value: &str) -> Result<PluginId> {
    PluginId::parse(value).ok_or_else(|| anyhow!("plugin id must use name@marketplace"))
}

fn canonical_plugin_dir(path: &Path) -> Result<PathBuf> {
    let path = std::fs::canonicalize(path)?;
    if !path.join("plugin.json").is_file() {
        return Err(anyhow!("{} does not contain plugin.json", path.display()));
    }
    Ok(path)
}

fn read_manifest(path: &Path) -> Result<PluginManifest> {
    serde_json::from_slice(&std::fs::read(path.join("plugin.json"))?)
        .context("failed to parse plugin.json")
}
