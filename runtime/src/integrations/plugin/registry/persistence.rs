//! Plugin registry state persistence.

use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::integrations::plugin::registry::types::PluginStatus;
use crate::integrations::plugin::{PluginError, PluginId};
use serde_json;
use std::collections::HashMap;

impl PluginRegistry {
    /// Save enabled/disabled state to `plugin_state.json`.
    pub fn save_state(&self) -> Result<(), PluginError> {
        let plugins = self.plugins.read().expect("plugin registry lock poisoned");
        let state: HashMap<String, serde_json::Value> = plugins
            .iter()
            .map(|(id, entry)| {
                let status_str = match entry.status {
                    PluginStatus::Enabled => "enabled",
                    PluginStatus::Degraded => "degraded",
                    PluginStatus::Disabled => "disabled",
                    PluginStatus::Error => "error",
                };
                (
                    id.to_string(),
                    serde_json::json!({
                        "status": status_str,
                        "errors": entry.load_errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    }),
                )
            })
            .collect();

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "version": 2,
            "plugins": state,
        }))?;

        if let Some(parent) = self.state_path().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let target = self.state_path();
        let temporary = target.with_extension("json.tmp");
        std::fs::write(&temporary, json)?;
        replace_state_file(&temporary, &target)?;
        Ok(())
    }
    /// Load enabled/disabled state from `plugin_state.json`.
    pub fn load_state(&self) -> Result<(), PluginError> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;

        let plugins = value.get("plugins").and_then(|v| v.as_object());

        if let Some(plugins) = plugins {
            for (id_str, status_val) in plugins {
                let status_str = status_val
                    .as_str()
                    .or_else(|| status_val.get("status").and_then(|value| value.as_str()));
                if let Some(id) = PluginId::parse(id_str)
                    && let Some(status_str) = status_str
                    && let Some(entry) =
                        self.plugins.write().expect("plugin registry lock poisoned").get_mut(&id)
                {
                    match status_str {
                        "enabled" => {
                            entry.status = PluginStatus::Enabled;
                            entry.plugin.enabled = true;
                        }
                        "degraded" => {
                            entry.status = PluginStatus::Degraded;
                            entry.plugin.enabled = true;
                        }
                        "disabled" => {
                            entry.status = PluginStatus::Disabled;
                            entry.plugin.enabled = false;
                        }
                        "error" => {
                            entry.status = PluginStatus::Error;
                        }
                        _ => {}
                    }
                    entry.load_errors = status_val
                        .get("errors")
                        .and_then(|value| value.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|value| value.as_str())
                        .map(|message| PluginError::Other(message.into()))
                        .collect();
                }
            }
        }

        Ok(())
    }
}

#[cfg(windows)]
fn replace_state_file(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    let backup = target.with_extension("json.backup");
    if backup.exists() {
        std::fs::remove_file(&backup)?;
    }
    if target.exists() {
        std::fs::rename(target, &backup)?;
    }
    if let Err(error) = std::fs::rename(source, target) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, target);
        }
        return Err(error);
    }
    if backup.exists() {
        let _ = std::fs::remove_file(backup);
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_state_file(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(source, target)
}
