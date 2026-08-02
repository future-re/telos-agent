//! Transactional plugin installation, upgrade, and uninstall operations.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::integrations::plugin::marketplace::{
    copy_directory, reset_directory, run_command_bounded,
};
use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::integrations::plugin::registry::types::{LoadedPlugin, PluginEntry, PluginStatus};
use crate::integrations::plugin::{
    MarketplaceEntry, MarketplaceRegistry, PluginError, PluginId, PluginSource,
};

#[cfg(test)]
thread_local! {
    static COMMIT_FAILURE_STEP: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(super) fn fail_commit_at(step: usize) {
    COMMIT_FAILURE_STEP.set(Some(step));
}

fn maybe_fail_commit(_step: usize) -> Result<(), PluginError> {
    #[cfg(test)]
    if COMMIT_FAILURE_STEP.get() == Some(_step) {
        COMMIT_FAILURE_STEP.set(None);
        return Err(PluginError::Other(format!("injected plugin commit failure at step {_step}")));
    }
    Ok(())
}

impl PluginRegistry {
    pub fn install(
        &self,
        marketplaces: &MarketplaceRegistry,
        id: &PluginId,
    ) -> Result<(), PluginError> {
        let _operation = self.operation_lock.lock().expect("plugin operation lock poisoned");
        if self.is_installed(id) {
            return Ok(());
        }
        self.install_or_upgrade(marketplaces, id, false)
    }

    pub fn upgrade(
        &self,
        marketplaces: &MarketplaceRegistry,
        id: &PluginId,
    ) -> Result<(), PluginError> {
        let _operation = self.operation_lock.lock().expect("plugin operation lock poisoned");
        let installed = self.get(id).ok_or_else(|| PluginError::PluginNotFound {
            plugin_id: id.to_string(),
            marketplace: id.marketplace.clone(),
        })?;
        if installed.status != PluginStatus::Disabled {
            return Err(PluginError::Other(format!("disable plugin `{id}` before replacing it")));
        }
        self.install_or_upgrade(marketplaces, id, true)
    }

    pub fn uninstall(&self, id: &PluginId) -> Result<(), PluginError> {
        let _operation = self.operation_lock.lock().expect("plugin operation lock poisoned");
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

        self.validate_planned_graph(marketplaces, &prepared, &change_ids)?;
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
            if !marketplaces.allows_dependency(&id.marketplace, &dependency_id.marketplace) {
                return Err(PluginError::DependencyUnsatisfied {
                    dependency: dependency_id.to_string(),
                    reason: crate::integrations::plugin::DependencyReason::NotAllowed,
                });
            }
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
        marketplaces: &MarketplaceRegistry,
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
                if !marketplaces.allows_dependency(&id.marketplace, &dependency_id.marketplace) {
                    return Err(PluginError::DependencyUnsatisfied {
                        dependency: dependency_id.to_string(),
                        reason: crate::integrations::plugin::DependencyReason::NotAllowed,
                    });
                }
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
            entry.strict,
        )?;
        let prepared = staging_root.join("prepared");
        copy_directory(&source_root, &prepared)?;
        if !prepared.join("plugin.json").is_file() {
            std::fs::write(
                prepared.join("plugin.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "manifestVersion": 2,
                    "name": entry.name,
                    "version": entry.version,
                    "description": entry.description,
                }))?,
            )?;
        }
        apply_manifest_override(&prepared, entry.manifest_override.as_ref())?;

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
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let backup_root = self.plugins_root.join(".trash").join(format!("transaction-{nonce}"));
        std::fs::create_dir_all(&backup_root)?;
        let mut completed = Vec::<CommittedChange>::new();

        for (index, (id, prepared)) in changes.iter_mut().enumerate() {
            if let Err(error) = maybe_fail_commit(index) {
                return Err(self.rollback_failure(&completed, error, Vec::new()));
            }
            let target = self.installed_dir().join(id.to_string());
            let backup = backup_root.join(id.to_string());
            let previous = self.get(id);
            if target.exists()
                && let Err(error) = std::fs::rename(&target, &backup)
            {
                return Err(self.rollback_failure(&completed, error.into(), Vec::new()));
            }
            if let Err(error) = std::fs::rename(&prepared.directory, &target) {
                let mut rollback_errors = Vec::new();
                if backup.exists()
                    && let Err(restore) = std::fs::rename(&backup, &target)
                {
                    rollback_errors
                        .push(format!("restoring `{}` failed: {restore}", target.display()));
                }
                return Err(self.rollback_failure(&completed, error.into(), rollback_errors));
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
            completed.push(CommittedChange { id: id.clone(), target, backup, previous });
        }

        if let Err(error) = maybe_fail_commit(changes.len()).and_then(|_| self.save_state()) {
            return Err(self.rollback_failure(&completed, error, Vec::new()));
        }
        if let Err(error) = std::fs::remove_dir_all(&backup_root) {
            tracing::warn!(path = %backup_root.display(), %error, "failed to clean plugin transaction backup");
        }
        Ok(())
    }

    fn rollback_failure(
        &self,
        completed: &[CommittedChange],
        original: PluginError,
        mut rollback_errors: Vec<String>,
    ) -> PluginError {
        if let Err(error) = self.rollback_committed(completed) {
            rollback_errors.push(error.to_string());
        }
        if rollback_errors.is_empty() {
            original
        } else {
            PluginError::Other(format!(
                "plugin transaction failed: {original}; rollback also failed: {}",
                rollback_errors.join("; ")
            ))
        }
    }

    fn rollback_committed(&self, completed: &[CommittedChange]) -> Result<(), PluginError> {
        let mut errors = Vec::new();
        for change in completed.iter().rev() {
            if change.target.exists()
                && let Err(error) = std::fs::remove_dir_all(&change.target)
            {
                errors.push(format!("removing `{}` failed: {error}", change.target.display()));
            }
            if change.backup.exists()
                && let Err(error) = std::fs::rename(&change.backup, &change.target)
            {
                errors.push(format!("restoring `{}` failed: {error}", change.target.display()));
            }
            self.remove(&change.id);
            if let Some(previous) = change.previous.clone() {
                restore_entry(self, previous);
            }
        }
        if let Err(error) = self.save_state() {
            errors.push(format!("restoring plugin state failed: {error}"));
        }
        if errors.is_empty() { Ok(()) } else { Err(PluginError::Other(errors.join("; "))) }
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

struct CommittedChange {
    id: PluginId,
    target: PathBuf,
    backup: PathBuf,
    previous: Option<PluginEntry>,
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

pub(super) fn restore_entry(registry: &PluginRegistry, entry: PluginEntry) {
    let id = entry.plugin.id.clone();
    registry.register(entry.plugin);
    if let Some(restored) =
        registry.plugins.write().expect("plugin registry lock poisoned").get_mut(&id)
    {
        restored.status = entry.status;
        restored.load_errors = entry.load_errors;
    }
}

fn materialize_source(
    source: &PluginSource,
    marketplace_base: Option<&Path>,
    staging: &Path,
    strict: bool,
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
            if strict && !path.join("plugin.json").is_file() {
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
            strict,
        ),
        PluginSource::Git { url, ref_, sha, path } => {
            materialize_git(url, ref_.as_deref(), sha.as_deref(), path.as_deref(), staging, strict)
        }
        PluginSource::Npm { package, version, registry } => {
            validate_package_spec(package, "npm")?;
            let specification = version
                .as_ref()
                .map(|version| format!("{package}@{version}"))
                .unwrap_or_else(|| package.clone());
            let mut command = std::process::Command::new("npm");
            command.args(["install", "--ignore-scripts", "--no-audit", "--no-fund", "--prefix"]);
            command.arg(staging).arg(&specification);
            if let Some(registry) = registry {
                command.arg("--registry").arg(registry);
            }
            run_install_command(command, "npm", package)?;
            let root = staging.join("node_modules").join(package);
            if strict { find_plugin_root(&root) } else { Ok(root) }
        }
        PluginSource::Pip { package, version, registry } => {
            validate_package_spec(package, "pip")?;
            let specification = version
                .as_ref()
                .map(|version| format!("{package}=={version}"))
                .unwrap_or_else(|| package.clone());
            let target = staging.join("package");
            let mut command = pip_command();
            command.args(["-m", "pip", "install", "--disable-pip-version-check", "--target"]);
            command.arg(&target).arg(&specification);
            if let Some(registry) = registry {
                command.arg("--index-url").arg(registry);
            }
            run_install_command(command, "pip", package)?;
            if strict { find_plugin_root(&target) } else { Ok(target) }
        }
    }
}

fn materialize_git(
    url: &str,
    reference: Option<&str>,
    sha: Option<&str>,
    subpath: Option<&str>,
    staging: &Path,
    strict: bool,
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
    if strict { find_plugin_root(&root) } else { Ok(root) }
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

fn validate_package_spec(package: &str, manager: &str) -> Result<(), PluginError> {
    if package.trim().is_empty()
        || package.starts_with('-')
        || package.contains("..")
        || package.contains('\\')
    {
        return Err(PluginError::Other(format!("unsafe {manager} package name `{package}`")));
    }
    Ok(())
}

fn pip_command() -> std::process::Command {
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("py");
        command.arg("-3");
        command
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("python3")
    }
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

fn run_install_command(
    mut command: std::process::Command,
    manager: &str,
    package: &str,
) -> Result<(), PluginError> {
    isolate_install_environment(&mut command);
    let build_error = |reason: String| match manager {
        "npm" => PluginError::NpmInstallFailed { package: package.into(), reason },
        "pip" => PluginError::PipInstallFailed { package: package.into(), reason },
        _ => PluginError::Other(format!("{manager} install failed for {package}: {reason}")),
    };
    let (status, stderr) =
        run_command_bounded(&mut command).map_err(|error| build_error(error.to_string()))?;
    if status.success() { Ok(()) } else { Err(build_error(stderr)) }
}

fn isolate_install_environment(command: &mut std::process::Command) {
    command.env_clear().envs(crate::config::platform_base_env());
}

fn find_plugin_root(root: &Path) -> Result<PathBuf, PluginError> {
    if root.join("plugin.json").is_file() {
        return Ok(root.to_path_buf());
    }
    let matches = walk_plugin_manifests(root, 4)?;
    if matches.len() == 1 {
        Ok(matches[0].parent().expect("manifest has parent").to_path_buf())
    } else {
        Err(PluginError::Other(format!(
            "expected exactly one plugin.json under {}, found {}",
            root.display(),
            matches.len()
        )))
    }
}

fn walk_plugin_manifests(root: &Path, depth: usize) -> Result<Vec<PathBuf>, PluginError> {
    if depth == 0 || !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        if entry.file_type()?.is_dir() {
            matches.extend(walk_plugin_manifests(&entry.path(), depth - 1)?);
        } else if entry.file_name() == "plugin.json" {
            matches.push(entry.path());
        }
    }
    Ok(matches)
}

fn apply_manifest_override(root: &Path, override_: Option<&Value>) -> Result<(), PluginError> {
    let Some(override_) = override_ else {
        return Ok(());
    };
    let path = root.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    merge_json(&mut manifest, override_);
    std::fs::write(path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}

fn merge_json(target: &mut Value, override_: &Value) {
    match (target, override_) {
        (Value::Object(target), Value::Object(override_)) => {
            for (key, value) in override_ {
                merge_json(target.entry(key).or_insert(Value::Null), value);
            }
        }
        (target, override_) => *target = override_.clone(),
    }
}
