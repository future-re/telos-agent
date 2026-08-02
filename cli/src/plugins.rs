use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::cli::{PluginCommand, SharedOptions};
use telos_agent::{
    Marketplace, MarketplaceEntry, MarketplaceRegistry, MarketplaceSource, PluginId,
    PluginManifest, PluginRegistry, PluginSource,
};

pub async fn run(command: PluginCommand, options: &SharedOptions) -> Result<()> {
    let cwd = options.cwd.clone().unwrap_or(std::env::current_dir()?);
    let project_root = crate::find_project_root(&cwd).unwrap_or(cwd);
    let plugins_root = project_root.join(".telos").join("plugins");
    let registry = load_registry(&plugins_root)?;
    let mut marketplaces = MarketplaceRegistry::new(&plugins_root);
    marketplaces.load()?;

    match command {
        PluginCommand::List => {
            let mut entries = registry.list_all();
            entries.sort_by_key(|entry| entry.plugin.id.to_string());
            for entry in entries {
                println!(
                    "{}\t{:?}\t{}\t{}",
                    entry.plugin.id,
                    entry.status,
                    entry.plugin.manifest.version,
                    marketplaces.source_status(&entry.plugin.id).as_str()
                );
                for error in entry.load_errors {
                    println!("  error: {error}");
                }
            }
        }
        PluginCommand::Inspect { id } => {
            let id = parse_id(&id)?;
            let entry =
                registry.get(&id).ok_or_else(|| anyhow!("plugin `{id}` is not installed"))?;
            let config = registry.resolved_config(&id);
            println!("id: {}", entry.plugin.id);
            println!("status: {:?}", entry.status);
            println!("version: {}", entry.plugin.manifest.version);
            println!("path: {}", entry.plugin.path.display());
            println!("source: {}", marketplaces.source_status(&id).as_str());
            match config {
                Ok(config) => {
                    println!("config: {}", serde_json::to_string_pretty(&config.redacted_values())?)
                }
                Err(error) => println!("config: {error}"),
            }
        }
        PluginCommand::Enable { id } => {
            let id = parse_id(&id)?;
            registry.enable(&id)?;
            registry.save_state()?;
            println!("enabled {id}");
        }
        PluginCommand::Disable { id } => {
            let id = parse_id(&id)?;
            registry.disable(&id)?;
            registry.save_state()?;
            println!("disabled {id}");
        }
        PluginCommand::Install { id } => {
            let id = parse_id(&id)?;
            refresh_marketplace(&registry, &mut marketplaces, &id)?;
            registry.install(&marketplaces, &id)?;
            println!("installed {id}");
        }
        PluginCommand::InstallLocal { path, marketplace } => {
            let path = canonical_plugin_dir(&path)?;
            let manifest = read_manifest(&path)?;
            let id = PluginId { name: manifest.name.clone(), marketplace: marketplace.clone() };
            let catalog_dir = plugins_root.join("local-marketplaces").join(&marketplace);
            std::fs::create_dir_all(&catalog_dir)?;
            let catalog_path = catalog_dir.join("marketplace.json");
            let mut catalog = if catalog_path.is_file() {
                serde_json::from_slice::<Marketplace>(&std::fs::read(&catalog_path)?)?
            } else {
                Marketplace {
                    name: marketplace.clone(),
                    owner: None,
                    plugins: Vec::new(),
                    allow_cross_marketplace_deps_on: None,
                }
            };
            let local_entry = MarketplaceEntry {
                name: manifest.name,
                description: manifest.description,
                version: manifest.version,
                source: PluginSource::Local { path },
                category: None,
                tags: Vec::new(),
                strict: true,
                manifest_override: None,
            };
            catalog.plugins.retain(|entry| entry.name != local_entry.name);
            catalog.plugins.push(local_entry);
            std::fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog)?)?;
            if marketplaces.get(&marketplace).is_some() {
                registry.refresh_marketplace(&mut marketplaces, &marketplace)?;
            } else {
                marketplaces.add(MarketplaceSource::Local { path: catalog_dir })?;
                marketplaces.save()?;
            }
            registry.install(&marketplaces, &id)?;
            println!("installed {id}");
        }
        PluginCommand::Upgrade { id } => {
            let id = parse_id(&id)?;
            refresh_marketplace(&registry, &mut marketplaces, &id)?;
            registry.upgrade(&marketplaces, &id)?;
            println!("upgraded {id}");
        }
        PluginCommand::Uninstall { id } => {
            let id = parse_id(&id)?;
            registry.uninstall(&id)?;
            println!("uninstalled {id}");
        }
        PluginCommand::Config { id, json } => {
            let id = parse_id(&id)?;
            let values: HashMap<String, serde_json::Value> =
                serde_json::from_str(&json).context("--json must be a JSON object")?;
            registry.set_config(&id, values)?;
            println!("configured {id}");
        }
        PluginCommand::ClearConfig { id } => {
            let id = parse_id(&id)?;
            registry.clear_config(&id)?;
            println!("cleared configuration for {id}");
        }
        PluginCommand::MarketplaceAddLocal { path, name } => {
            let name = marketplaces
                .add_named(MarketplaceSource::Local { path: std::fs::canonicalize(path)? }, name)?;
            marketplaces.save()?;
            println!("added marketplace {name}");
        }
        PluginCommand::MarketplaceAddUrl { url, name } => {
            let name = marketplaces.add_named(MarketplaceSource::Url { url }, name)?;
            registry.refresh_marketplace(&mut marketplaces, &name)?;
            println!("added marketplace {name}");
        }
        PluginCommand::MarketplaceAddGithub { repo, ref_, path, name } => {
            let name =
                marketplaces.add_named(MarketplaceSource::GitHub { repo, ref_, path }, name)?;
            registry.refresh_marketplace(&mut marketplaces, &name)?;
            println!("added marketplace {name}");
        }
        PluginCommand::MarketplaceAddGit { url, ref_, path, name } => {
            let name = marketplaces.add_named(MarketplaceSource::Git { url, ref_, path }, name)?;
            registry.refresh_marketplace(&mut marketplaces, &name)?;
            println!("added marketplace {name}");
        }
        PluginCommand::MarketplaceAddNpm { package, name } => {
            let name = marketplaces.add_named(MarketplaceSource::Npm { package }, name)?;
            registry.refresh_marketplace(&mut marketplaces, &name)?;
            println!("added marketplace {name}");
        }
        PluginCommand::MarketplaceRefresh { name } => {
            let report = registry.refresh_marketplace(&mut marketplaces, &name)?;
            for id in report.orphaned {
                println!("retained orphaned plugin {id}");
            }
            println!("refreshed marketplace {name}");
        }
        PluginCommand::MarketplaceRemove { name } => {
            registry.remove_marketplace(&mut marketplaces, &name)?;
            println!("removed marketplace {name}");
        }
        PluginCommand::MarketplaceSearch { query } => {
            let query = query.to_lowercase();
            let mut matches = marketplace_entries(&marketplaces, None)?
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
            let mut entries = marketplace_entries(&marketplaces, name.as_deref())?;
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            for (id, entry) in entries {
                println!("{id}\t{}\t{}", entry.version, entry.description.as_deref().unwrap_or(""));
            }
        }
        PluginCommand::MarketplaceList => {
            let mut names = marketplaces.names().into_iter().cloned().collect::<Vec<_>>();
            names.sort();
            for name in names {
                let count = marketplaces
                    .get(&name)
                    .map(|marketplace| marketplace.plugins.len())
                    .unwrap_or(0);
                println!("{name}\t{count} plugins");
            }
        }
    }
    Ok(())
}

fn load_registry(root: &Path) -> Result<PluginRegistry> {
    std::fs::create_dir_all(root.join("installed"))?;
    let registry = PluginRegistry::new(root);
    registry.discover_installed()?;
    registry.load_state()?;
    registry.load_config()?;
    Ok(registry)
}

fn refresh_marketplace(
    registry: &PluginRegistry,
    marketplaces: &mut MarketplaceRegistry,
    id: &PluginId,
) -> Result<()> {
    registry.refresh_marketplace(marketplaces, &id.marketplace)?;
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
