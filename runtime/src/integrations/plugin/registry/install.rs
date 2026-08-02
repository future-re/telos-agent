//! Transactional plugin installation, upgrade, and uninstall operations.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::integrations::plugin::marketplace::{
    copy_directory, reset_directory, run_command_bounded,
};
use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::integrations::plugin::registry::types::{LoadedPlugin, PluginStatus};
use crate::integrations::plugin::{
    MarketplaceEntry, MarketplaceRegistry, PluginError, PluginId, PluginSource,
};

impl PluginRegistry {
    pub(crate) fn install(
        &self,
        marketplaces: &MarketplaceRegistry,
        id: &PluginId,
    ) -> Result<(), PluginError> {
        if self.is_installed(id) {
            return Ok(());
        }
        self.install_or_upgrade(marketplaces, id, false)
    }

    pub(crate) fn upgrade(
        &self,
        marketplaces: &MarketplaceRegistry,
        id: &PluginId,
    ) -> Result<(), PluginError> {
        let installed = self.get(id).ok_or_else(|| PluginError::PluginNotFound {
            plugin_id: id.to_string(),
            marketplace: id.marketplace.clone(),
        })?;
        if installed.status != PluginStatus::Disabled {
            return Err(PluginError::Other(format!("disable plugin `{id}` before replacing it")));
        }
        self.install_or_upgrade(marketplaces, id, true)
    }

    pub(crate) fn uninstall(&self, id: &PluginId) -> Result<(), PluginError> {
        self.uninstall_locked(id)
    }

    fn uninstall_locked(&self, id: &PluginId) -> Result<(), PluginError> {
        let dependents =
            self.list_all()
                .into_iter()
                .filter(|entry| {
                    entry.plugin.id != *id
                        && entry.plugin.manifest.dependencies.iter().any(|dependency| {
                            dependency.resolve(&entry.plugin.id.marketplace) == *id
                        })
                })
                .map(|entry| entry.plugin.id)
                .collect::<Vec<_>>();
        if !dependents.is_empty() {
            return Err(PluginError::DependencyRequiredBy { dependency: id.clone(), dependents });
        }
        let entry = self.get(id).ok_or_else(|| PluginError::PluginNotFound {
            plugin_id: id.to_string(),
            marketplace: id.marketplace.clone(),
        })?;
        if matches!(
            entry.status,
            crate::integrations::plugin::PluginStatus::Enabled
                | crate::integrations::plugin::PluginStatus::Degraded
        ) {
            return Err(PluginError::Other(format!(
                "disable plugin `{id}` before uninstalling it"
            )));
        }
        let trash = self.plugins_root.join(".trash").join(id.to_string());
        if trash.exists() {
            std::fs::remove_dir_all(&trash)?;
        }
        if entry.plugin.path.exists() {
            if let Some(parent) = trash.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&entry.plugin.path, &trash)?;
        }
        self.remove(id);
        if let Err(error) = self.save_state() {
            if trash.exists() {
                let _ = std::fs::rename(&trash, &entry.plugin.path);
            }
            self.register(entry.plugin);
            return Err(error);
        }
        if let Err(error) = self.clear_config(id) {
            if trash.exists() {
                let _ = std::fs::rename(&trash, &entry.plugin.path);
            }
            self.register(entry.plugin);
            let _ = self.save_state();
            return Err(error);
        }
        if trash.exists()
            && let Err(error) = std::fs::remove_dir_all(&trash)
        {
            tracing::warn!(path = %trash.display(), %error, "failed to clean plugin uninstall trash");
        }
        Ok(())
    }

    fn install_or_upgrade(
        &self,
        marketplaces: &MarketplaceRegistry,
        root: &PluginId,
        upgrading: bool,
    ) -> Result<(), PluginError> {
        let mut prepared = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = Vec::new();
        let mut requirements = Vec::new();
        self.prepare_closure(
            marketplaces,
            root,
            &mut seen,
            &mut stack,
            &mut requirements,
            &mut prepared,
        )?;

        let mut candidate_versions = self
            .list_all()
            .into_iter()
            .map(|entry| (entry.plugin.id, entry.plugin.manifest.version))
            .collect::<HashMap<_, _>>();
        candidate_versions.extend(
            prepared
                .iter()
                .map(|(id, plugin)| (id.clone(), plugin.plugin.manifest.version.clone())),
        );
        for requirement in &requirements {
            let actual = candidate_versions.get(&requirement.plugin).ok_or_else(|| {
                PluginError::DependencyUnsatisfied {
                    dependency: requirement.plugin.to_string(),
                    reason: crate::integrations::plugin::DependencyReason::NotFound,
                }
            })?;
            if !requirement.version.matches(actual) {
                return Err(PluginError::DependencyVersionConflict {
                    plugin: Box::new(requirement.plugin.clone()),
                    required: Box::new(requirement.version.clone()),
                    actual: Box::new(actual.clone()),
                    required_by: Box::new(requirement.required_by.clone()),
                });
            }
        }

        let change_ids = prepared
            .iter()
            .filter_map(|(id, candidate)| {
                let installed = self.get(id);
                let changed = installed.as_ref().is_none_or(|entry| {
                    entry.plugin.manifest.version != candidate.plugin.manifest.version
                });
                (changed || (upgrading && id == root)).then_some(id.clone())
            })
            .collect::<HashSet<_>>();

        for id in &change_ids {
            if let Some(entry) = self.get(id)
                && entry.status != PluginStatus::Disabled
            {
                return Err(PluginError::Other(format!(
                    "disable plugin `{id}` before replacing it"
                )));
            }
        }

        self.validate_planned_graph(&prepared, &change_ids)?;
        for (id, candidate) in &prepared {
            if change_ids.contains(id) && self.is_installed(id) {
                self.validate_config_for_manifest(id, &candidate.plugin.manifest)?;
            }
        }

        let changes =
            prepared.into_iter().filter(|(id, _)| change_ids.contains(id)).collect::<Vec<_>>();
        self.commit_prepared_batch(changes)
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_closure(
        &self,
        marketplaces: &MarketplaceRegistry,
        id: &PluginId,
        seen: &mut HashSet<PluginId>,
        stack: &mut Vec<PluginId>,
        requirements: &mut Vec<VersionRequirement>,
        prepared: &mut Vec<(PluginId, PreparedPlugin)>,
    ) -> Result<(), PluginError> {
        if seen.contains(id) {
            return Ok(());
        }
        if let Some(index) = stack.iter().position(|candidate| candidate == id) {
            let mut cycle = stack[index..].to_vec();
            cycle.push(id.clone());
            return Err(PluginError::CircularDependency { cycle });
        }
        let entry = marketplaces.plugin_entry(id).ok_or_else(|| PluginError::PluginNotFound {
            plugin_id: id.to_string(),
            marketplace: id.marketplace.clone(),
        })?;
        let candidate = self.prepare_entry(marketplaces, id, &entry)?;
        stack.push(id.clone());
        for dependency in &candidate.plugin.manifest.dependencies {
            let dependency_id = dependency.resolve(&id.marketplace);
            requirements.push(VersionRequirement {
                plugin: dependency_id.clone(),
                version: dependency.version.clone(),
                required_by: id.clone(),
            });
            let installed_satisfies = self
                .get(&dependency_id)
                .is_some_and(|entry| dependency.version.matches(&entry.plugin.manifest.version));
            if !installed_satisfies {
                self.prepare_closure(
                    marketplaces,
                    &dependency_id,
                    seen,
                    stack,
                    requirements,
                    prepared,
                )?;
            }
        }
        stack.pop();
        seen.insert(id.clone());
        prepared.push((id.clone(), candidate));
        Ok(())
    }

    fn validate_planned_graph(
        &self,
        prepared: &[(PluginId, PreparedPlugin)],
        change_ids: &HashSet<PluginId>,
    ) -> Result<(), PluginError> {
        let candidates = prepared
            .iter()
            .map(|(id, candidate)| (id.clone(), candidate.plugin.manifest.clone()))
            .collect::<HashMap<_, _>>();
        let installed = self
            .list_all()
            .into_iter()
            .map(|entry| (entry.plugin.id.clone(), entry.plugin.manifest))
            .collect::<HashMap<_, _>>();
        let mut combined = installed.clone();
        for id in change_ids {
            if let Some(manifest) = candidates.get(id) {
                combined.insert(id.clone(), manifest.clone());
            }
        }
        for (id, manifest) in &combined {
            for dependency in &manifest.dependencies {
                let dependency_id = dependency.resolve(&id.marketplace);
                let actual = combined.get(&dependency_id).ok_or_else(|| {
                    PluginError::DependencyUnsatisfied {
                        dependency: dependency_id.to_string(),
                        reason: crate::integrations::plugin::DependencyReason::NotFound,
                    }
                })?;
                if !dependency.version.matches(&actual.version) {
                    return Err(PluginError::DependencyVersionConflict {
                        plugin: Box::new(dependency_id),
                        required: Box::new(dependency.version.clone()),
                        actual: Box::new(actual.version.clone()),
                        required_by: Box::new(id.clone()),
                    });
                }
            }
        }
        validate_manifest_cycles(&combined)
    }

    fn prepare_entry(
        &self,
        marketplaces: &MarketplaceRegistry,
        id: &PluginId,
        entry: &MarketplaceEntry,
    ) -> Result<PreparedPlugin, PluginError> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let staging_root = self.plugins_root.join(".staging").join(format!("{}-{nonce}", id.name));
        reset_directory(&staging_root)?;
        let mut cleanup = CleanupDirectory(Some(staging_root.clone()));
        let source_staging = staging_root.join("source");
        std::fs::create_dir_all(&source_staging)?;
        let source_root = materialize_source(
            &entry.source,
            marketplaces.source_base(&id.marketplace).as_deref(),
            &source_staging,
        )?;
        let prepared = staging_root.join("prepared");
        copy_directory(&source_root, &prepared)?;
        if !prepared.join("plugin.json").is_file() {
            return Err(PluginError::ManifestNotFound { path: prepared.join("plugin.json") });
        }

        let validation_dir = staging_root.join("validate").join(id.to_string());
        if let Some(parent) = validation_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if validation_dir.exists() {
            std::fs::remove_dir_all(&validation_dir)?;
        }
        std::fs::rename(&prepared, &validation_dir)?;
        let mut plugin = self.load_plugin_from_dir(&validation_dir)?;
        if plugin.manifest.version != entry.version {
            return Err(PluginError::VersionMismatch {
                plugin: Box::new(id.clone()),
                declared: Box::new(entry.version.clone()),
                actual: Box::new(plugin.manifest.version.clone()),
            });
        }
        plugin.source = entry.source.clone();
        cleanup.disarm();
        Ok(PreparedPlugin { plugin, directory: validation_dir, staging_root })
    }

    fn commit_prepared_batch(
        &self,
        mut changes: Vec<(PluginId, PreparedPlugin)>,
    ) -> Result<(), PluginError> {
        if changes.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(self.installed_dir())?;
        for (id, prepared) in &mut changes {
            let target = self.installed_dir().join(id.to_string());
            let backup = self.plugins_root.join(".trash").join(id.to_string());
            if let Some(parent) = backup.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if backup.exists() {
                std::fs::remove_dir_all(&backup)?;
            }
            if target.exists() {
                std::fs::rename(&target, &backup)?;
            }
            if let Err(error) = std::fs::rename(&prepared.directory, &target) {
                if backup.exists() {
                    std::fs::rename(&backup, &target)?;
                }
                return Err(error.into());
            }
            prepared.plugin.path = target.clone();
            prepared.plugin.enabled = false;
            self.register(prepared.plugin.clone());
            if let Some(entry) =
                self.plugins.write().expect("plugin registry lock poisoned").get_mut(id)
            {
                entry.status = if entry.plugin.enabled {
                    PluginStatus::Enabled
                } else {
                    PluginStatus::Disabled
                };
                entry.load_errors.clear();
            }
            if backup.exists()
                && let Err(error) = std::fs::remove_dir_all(&backup)
            {
                tracing::warn!(path = %backup.display(), %error, "failed to clean plugin backup");
            }
        }
        Ok(())
    }
}

struct VersionRequirement {
    plugin: PluginId,
    version: semver::VersionReq,
    required_by: PluginId,
}

fn validate_manifest_cycles(
    manifests: &HashMap<PluginId, crate::integrations::plugin::PluginManifest>,
) -> Result<(), PluginError> {
    fn visit(
        id: &PluginId,
        manifests: &HashMap<PluginId, crate::integrations::plugin::PluginManifest>,
        stack: &mut Vec<PluginId>,
        visited: &mut HashSet<PluginId>,
    ) -> Result<(), PluginError> {
        if let Some(index) = stack.iter().position(|candidate| candidate == id) {
            let mut cycle = stack[index..].to_vec();
            cycle.push(id.clone());
            return Err(PluginError::CircularDependency { cycle });
        }
        if !visited.insert(id.clone()) {
            return Ok(());
        }
        stack.push(id.clone());
        if let Some(manifest) = manifests.get(id) {
            for dependency in &manifest.dependencies {
                visit(&dependency.resolve(&id.marketplace), manifests, stack, visited)?;
            }
        }
        stack.pop();
        Ok(())
    }

    let mut visited = HashSet::new();
    for id in manifests.keys() {
        visit(id, manifests, &mut Vec::new(), &mut visited)?;
    }
    Ok(())
}

struct PreparedPlugin {
    plugin: LoadedPlugin,
    directory: PathBuf,
    staging_root: PathBuf,
}

impl PreparedPlugin {
    fn cleanup(&self) {
        if self.staging_root.exists() {
            let _ = std::fs::remove_dir_all(&self.staging_root);
        }
        if let Some(validation_root) = self.directory.parent()
            && validation_root.exists()
        {
            let _ = std::fs::remove_dir_all(validation_root);
        }
    }
}

impl Drop for PreparedPlugin {
    fn drop(&mut self) {
        self.cleanup();
    }
}

struct CleanupDirectory(Option<PathBuf>);

impl CleanupDirectory {
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for CleanupDirectory {
    fn drop(&mut self) {
        if let Some(path) = &self.0
            && path.exists()
        {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn materialize_source(
    source: &PluginSource,
    marketplace_base: Option<&Path>,
    staging: &Path,
) -> Result<PathBuf, PluginError> {
    match source {
        PluginSource::Local { path } => {
            let (path, containment_root) = if path.is_absolute() {
                (path.clone(), None)
            } else {
                let base = marketplace_base.ok_or_else(|| {
                    PluginError::Other("relative local source has no base".into())
                })?;
                (base.join(path), Some(base))
            };
            let path = std::fs::canonicalize(&path)?;
            if let Some(base) = containment_root {
                let base = std::fs::canonicalize(base)?;
                if !path.starts_with(&base) {
                    return Err(PluginError::Other(format!(
                        "local plugin source {} escapes marketplace root {}",
                        path.display(),
                        base.display()
                    )));
                }
            }
            if !path.join("plugin.json").is_file() {
                return Err(PluginError::ManifestNotFound { path: path.join("plugin.json") });
            }
            Ok(path)
        }
        PluginSource::GitHub { repo, ref_, sha, path } => materialize_git(
            &format!("https://github.com/{repo}.git"),
            ref_.as_deref(),
            sha.as_deref(),
            path.as_deref(),
            staging,
        ),
    }
}

fn materialize_git(
    url: &str,
    reference: Option<&str>,
    sha: Option<&str>,
    subpath: Option<&str>,
    staging: &Path,
) -> Result<PathBuf, PluginError> {
    let checkout = staging.join("repository");
    let mut clone = std::process::Command::new("git");
    clone.args(["clone", "--depth", "1"]);
    if let Some(reference) = reference {
        clone.args(["--branch", reference]);
    }
    clone.arg(url).arg(&checkout);
    run_git(clone, url)?;
    if let Some(sha) = sha {
        let mut fetch = std::process::Command::new("git");
        fetch.arg("-C").arg(&checkout).args(["fetch", "--depth", "1", "origin", sha]);
        run_git(fetch, url)?;
        let mut checkout_command = std::process::Command::new("git");
        checkout_command.arg("-C").arg(&checkout).args(["checkout", "--detach", sha]);
        run_git(checkout_command, url)?;
    }
    let root = if let Some(subpath) = subpath {
        let subpath = safe_relative_path(subpath)?;
        let root = checkout.join(subpath);
        let canonical_checkout = std::fs::canonicalize(&checkout)?;
        let canonical_root = std::fs::canonicalize(&root)?;
        if !canonical_root.starts_with(&canonical_checkout) {
            return Err(PluginError::Other(format!(
                "git plugin subpath `{}` escapes repository root",
                subpath.display()
            )));
        }
        canonical_root
    } else {
        checkout
    };
    if root.join("plugin.json").is_file() {
        Ok(root)
    } else {
        Err(PluginError::ManifestNotFound { path: root.join("plugin.json") })
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
        return Err(PluginError::Other(format!("unsafe relative path `{}`", path.display())));
    }
    Ok(path)
}

fn run_git(mut command: std::process::Command, url: &str) -> Result<(), PluginError> {
    isolate_install_environment(&mut command);
    let (status, stderr) = run_command_bounded(&mut command).map_err(|error| {
        PluginError::GitCloneFailed { url: url.into(), reason: error.to_string() }
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(PluginError::GitCloneFailed { url: url.into(), reason: stderr })
    }
}

fn isolate_install_environment(command: &mut std::process::Command) {
    command.env_clear().envs(crate::config::platform_base_env());
}
