//! Plugin registry state persistence.

use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::integrations::plugin::registry::types::PluginStatus;
use crate::integrations::plugin::{PluginError, PluginId};
use serde_json;
use std::collections::HashMap;

impl PluginRegistry {
    /// Save enabled/disabled state to `plugin_state.json`.
    pub fn save_state(&self) -> Result<(), PluginError> {
        let state: HashMap<String, String> = self
            .plugins
            .iter()
            .map(|(id, entry)| {
                let status_str = match entry.status {
                    PluginStatus::Enabled => "enabled",
                    PluginStatus::Degraded => "degraded",
                    PluginStatus::Disabled => "disabled",
                    PluginStatus::Error => "error",
                };
                (id.to_string(), status_str.to_string())
            })
            .collect();

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "plugins": state,
        }))?;

        if let Some(parent) = self.state_path().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(self.state_path(), json)?;
        Ok(())
    }
    /// Load enabled/disabled state from `plugin_state.json`.
    pub fn load_state(&mut self) -> Result<(), PluginError> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;

        let plugins = value.get("plugins").and_then(|v| v.as_object());

        if let Some(plugins) = plugins {
            for (id_str, status_val) in plugins {
                if let Some(id) = PluginId::parse(id_str)
                    && let Some(status_str) = status_val.as_str()
                    && let Some(entry) = self.plugins.get_mut(&id)
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
                }
            }
        }

        Ok(())
    }
}
