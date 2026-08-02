//! Plugin registry state persistence.

use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::integrations::plugin::registry::types::PluginStatus;
use crate::integrations::plugin::{PluginError, PluginId};
use serde_json;
use std::collections::HashMap;

impl PluginRegistry {
    /// Save enabled/disabled state to the shared `state.json`.
    pub(crate) fn save_state(&self) -> Result<(), PluginError> {
        let plugins = self.plugins.read().expect("plugin registry lock poisoned");
        let state: HashMap<String, &'static str> = plugins
            .iter()
            .map(|(id, entry)| {
                let status_str = match entry.status {
                    PluginStatus::Enabled | PluginStatus::Degraded => "enabled",
                    PluginStatus::Disabled | PluginStatus::Error => "disabled",
                };
                (id.to_string(), status_str)
            })
            .collect();

        crate::integrations::plugin::state::write_section(
            &self.state_path(),
            "plugins",
            serde_json::to_value(state)?,
        )
    }
    /// Load enabled/disabled state from the shared `state.json`.
    pub(crate) fn load_state(&self) -> Result<(), PluginError> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(());
        }

        let value = crate::integrations::plugin::state::read(&path)?;

        let plugins = value.get("plugins").and_then(|v| v.as_object());

        if let Some(plugins) = plugins {
            for (id_str, status_val) in plugins {
                let status_str = status_val.as_str();
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
                        "disabled" => {
                            entry.status = PluginStatus::Disabled;
                            entry.plugin.enabled = false;
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }
}
