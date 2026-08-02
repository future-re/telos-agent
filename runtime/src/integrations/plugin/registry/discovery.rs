//! Plugin discovery from installed directories.

use crate::integrations::plugin::manifest::PluginManifest;
use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::integrations::plugin::registry::types::LoadedPlugin;
use crate::integrations::plugin::{PluginError, PluginId, PluginSource};
use std::path::{Path, PathBuf};

impl PluginRegistry {
    /// Scan the installed directory and load all plugins found there.
    ///
    /// Each subdirectory that contains a `plugin.json` is loaded.
    /// Plugins are NOT auto-enabled — state is restored from `plugin_state.json`.
    pub fn discover_installed(&self) -> Result<Vec<PluginId>, PluginError> {
        let installed_dir = self.installed_dir();
        if !installed_dir.exists() {
            return Ok(Vec::new());
        }

        let mut discovered = Vec::new();
        let entries = std::fs::read_dir(&installed_dir)?;

        for entry in entries {
            let entry = entry?;
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }

            let manifest_path = dir.join("plugin.json");
            if !manifest_path.exists() {
                continue;
            }

            match self.load_plugin_from_dir(&dir) {
                Ok(plugin) => {
                    let id = plugin.id.clone();
                    discovered.push(id.clone());
                    self.register(plugin);
                }
                Err(err) => {
                    tracing::warn!(
                        dir = %dir.display(),
                        error = %err,
                        "failed to load plugin from installed directory"
                    );
                }
            }
        }

        Ok(discovered)
    }
    /// Load a single plugin from a directory containing plugin.json.
    pub(crate) fn load_plugin_from_dir(&self, dir: &Path) -> Result<LoadedPlugin, PluginError> {
        let manifest_path = dir.join("plugin.json");
        let content =
            std::fs::read_to_string(&manifest_path).map_err(|e| PluginError::ManifestParse {
                path: manifest_path.clone(),
                reason: format!("failed to read: {e}"),
            })?;

        let raw: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| PluginError::ManifestParse {
                path: manifest_path.clone(),
                reason: format!("invalid JSON: {e}"),
            })?;
        if raw.get("hooks").is_some() || raw.get("interceptors").is_some() {
            return Err(PluginError::ManifestValidation {
                errors: vec![
                    "the `hooks` and `interceptors` plugin fields were removed; use `policies`"
                        .into(),
                ],
            });
        }
        let manifest: PluginManifest =
            serde_json::from_value(raw).map_err(|e| PluginError::ManifestParse {
                path: manifest_path.clone(),
                reason: format!("invalid manifest: {e}"),
            })?;

        let mut validation_errors = Vec::new();
        if manifest.name.trim().is_empty() {
            validation_errors.push("name must not be empty".into());
        } else if !crate::integrations::plugin::is_valid_id_part(&manifest.name) {
            validation_errors.push(
                "name may contain only ASCII letters, digits, dots, hyphens, and underscores"
                    .into(),
            );
        }
        if manifest.manifest_version != 2 {
            validation_errors.push(format!(
                "unsupported manifestVersion {}; expected 2",
                manifest.manifest_version
            ));
        }
        if let Some(policies) = &manifest.policies {
            validation_errors.extend(policies.validate());
        }
        for (index, dependency) in manifest.dependencies.iter().enumerate() {
            if !crate::integrations::plugin::is_valid_id_part(&dependency.name) {
                validation_errors
                    .push(format!("dependencies[{index}].name is not a valid plugin name"));
            }
            if dependency.marketplace.as_ref().is_some_and(|marketplace| {
                !crate::integrations::plugin::is_valid_id_part(marketplace)
            }) {
                validation_errors.push(format!(
                    "dependencies[{index}].marketplace is not a valid marketplace name"
                ));
            }
        }
        validation_errors
            .extend(crate::integrations::plugin::config::validate_manifest_config(&manifest));
        if !validation_errors.is_empty() {
            return Err(PluginError::ManifestValidation { errors: validation_errors });
        }

        // Determine marketplace from the parent directory name
        // installed/<name>@<marketplace>/plugin.json
        let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("unknown@unknown");
        let id = PluginId::parse(dir_name).unwrap_or_else(|| PluginId {
            name: manifest.name.clone(),
            marketplace: "unknown".into(),
        });
        if id.name != manifest.name {
            return Err(PluginError::ManifestValidation {
                errors: vec![format!(
                    "manifest name `{}` does not match installed directory plugin id `{}`",
                    manifest.name, id.name
                )],
            });
        }

        // Resolve component paths
        let resolve = |paths: &Option<Vec<String>>| -> Result<Vec<PathBuf>, PluginError> {
            paths
                .as_ref()
                .map(|paths| paths.iter().map(|path| resolve_component_path(dir, path)).collect())
                .unwrap_or_else(|| Ok(Vec::new()))
        };

        // Find .json tool files in the tools/ directory
        let mut resolved_tools = resolve(&manifest.tools)?;
        let tools_dir = dir.join("tools");
        if tools_dir.exists()
            && tools_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&tools_dir)
        {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|ext| ext == "json") {
                    resolved_tools.push(p);
                }
            }
        }
        resolved_tools.sort();
        resolved_tools.dedup();

        let resolved_skills = resolve(&manifest.skills)?;
        let resolved_agents = resolve(&manifest.agents)?;
        let resolved_prompt_sections = resolve(&manifest.prompt_sections)?;
        let resolved_output_styles = resolve(&manifest.output_styles)?;

        Ok(LoadedPlugin {
            id,
            manifest,
            path: dir.to_path_buf(),
            source: PluginSource::Local { path: dir.to_path_buf() },
            enabled: false, // will be set by state restoration
            is_builtin: false,
            resolved_tools,
            resolved_skills,
            resolved_agents,
            resolved_prompt_sections,
            resolved_output_styles,
        })
    }
}

fn resolve_component_path(root: &Path, declared: &str) -> Result<PathBuf, PluginError> {
    let path = Path::new(declared);
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
        return Err(PluginError::ManifestValidation {
            errors: vec![format!("component path `{declared}` escapes the plugin root")],
        });
    }
    let joined = root.join(path);
    if joined.exists() {
        let canonical_root = std::fs::canonicalize(root)?;
        let canonical = std::fs::canonicalize(&joined)?;
        if !canonical.starts_with(&canonical_root) {
            return Err(PluginError::ManifestValidation {
                errors: vec![format!("component path `{declared}` resolves outside plugin root")],
            });
        }
        Ok(canonical)
    } else {
        Ok(joined)
    }
}
