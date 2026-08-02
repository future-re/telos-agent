//! Validated, per-plugin user configuration storage and runtime projection.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::integrations::plugin::manifest::{ConfigOptionType, PluginManifest, UserConfigOption};
use crate::integrations::plugin::{PluginError, PluginId};

#[derive(Clone, Default)]
pub struct ResolvedPluginConfig {
    values: HashMap<String, Value>,
    sensitive_keys: Vec<String>,
}

impl std::fmt::Debug for ResolvedPluginConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedPluginConfig")
            .field("values", &self.redacted_values())
            .finish()
    }
}

impl ResolvedPluginConfig {
    pub fn values(&self) -> &HashMap<String, Value> {
        &self.values
    }

    pub fn redacted_values(&self) -> HashMap<String, Value> {
        self.values
            .iter()
            .map(|(key, value)| {
                let value = if self.sensitive_keys.contains(key) {
                    Value::String("[REDACTED]".into())
                } else {
                    value.clone()
                };
                (key.clone(), value)
            })
            .collect()
    }

    pub fn command_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert(
            "TELOS_PLUGIN_CONFIG".into(),
            serde_json::to_string(&self.values).unwrap_or_else(|_| "{}".into()),
        );
        for (key, value) in &self.values {
            env.insert(format!("TELOS_PLUGIN_{}", normalize_env_key(key)), scalar_env_value(value));
        }
        env
    }

    pub fn render_template(&self, template: &str) -> String {
        let mut rendered = template.to_string();
        for (key, value) in &self.values {
            if self.sensitive_keys.contains(key) {
                continue;
            }
            rendered = rendered.replace(&format!("${{CONFIG:{key}}}"), &scalar_env_value(value));
        }
        rendered
    }
}

#[derive(Clone)]
pub struct PluginConfigStore {
    path: PathBuf,
    values: HashMap<PluginId, HashMap<String, Value>>,
}

impl std::fmt::Debug for PluginConfigStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginConfigStore")
            .field("path", &self.path)
            .field("plugin_count", &self.values.len())
            .field("values", &"[REDACTED]")
            .finish()
    }
}

impl PluginConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), values: HashMap::new() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&mut self) -> Result<(), PluginError> {
        if !self.path.exists() {
            return Ok(());
        }
        let root = crate::integrations::plugin::state::read(&self.path)?;
        let mut values = HashMap::new();
        if let Some(plugins) = root.get("config").and_then(Value::as_object) {
            for (id, config) in plugins {
                let Some(id) = PluginId::parse(id) else {
                    continue;
                };
                let Some(config) = config.as_object() else {
                    continue;
                };
                values.insert(id, config.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
            }
        }
        self.values = values;
        Ok(())
    }

    pub fn save(&self) -> Result<(), PluginError> {
        let plugins: HashMap<String, &HashMap<String, Value>> =
            self.values.iter().map(|(id, values)| (id.to_string(), values)).collect();
        crate::integrations::plugin::state::write_section(
            &self.path,
            "config",
            serde_json::to_value(plugins)?,
        )
    }

    pub fn set(
        &mut self,
        id: &PluginId,
        manifest: &PluginManifest,
        values: HashMap<String, Value>,
    ) -> Result<(), PluginError> {
        let previous = self.values.get(id).cloned();
        let mut merged = previous.clone().unwrap_or_default();
        merged.extend(values);
        validate_user_values(id, manifest, &merged, false)?;
        self.values.insert(id.clone(), merged);
        if let Err(error) = self.save() {
            if let Some(previous) = previous {
                self.values.insert(id.clone(), previous);
            } else {
                self.values.remove(id);
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn clear(&mut self, id: &PluginId) -> Result<(), PluginError> {
        let previous = self.values.remove(id);
        if let Err(error) = self.save() {
            if let Some(previous) = previous {
                self.values.insert(id.clone(), previous);
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn get(&self, id: &PluginId) -> Option<&HashMap<String, Value>> {
        self.values.get(id)
    }

    pub(crate) fn validate_for_manifest(
        &self,
        id: &PluginId,
        manifest: &PluginManifest,
    ) -> Result<(), PluginError> {
        self.resolve(id, manifest).map(|_| ())
    }

    pub fn resolve(
        &self,
        id: &PluginId,
        manifest: &PluginManifest,
    ) -> Result<ResolvedPluginConfig, PluginError> {
        let user_values = self.values.get(id).cloned().unwrap_or_default();
        validate_user_values(id, manifest, &user_values, true)?;

        let mut values = manifest.settings.clone().unwrap_or_default();
        let mut sensitive_keys = Vec::new();
        if let Some(schema) = &manifest.user_config {
            for (key, option) in schema {
                if let Some(default) = &option.default {
                    values.entry(key.clone()).or_insert_with(|| default.clone());
                }
                if option.sensitive {
                    sensitive_keys.push(key.clone());
                }
            }
        }
        values.extend(user_values);
        Ok(ResolvedPluginConfig { values, sensitive_keys })
    }
}

fn validate_user_values(
    id: &PluginId,
    manifest: &PluginManifest,
    values: &HashMap<String, Value>,
    require_complete: bool,
) -> Result<(), PluginError> {
    validate_values(id, manifest, values, require_complete)
}

fn validate_values(
    id: &PluginId,
    manifest: &PluginManifest,
    values: &HashMap<String, Value>,
    require_complete: bool,
) -> Result<(), PluginError> {
    let schema = manifest.user_config.as_ref();
    let mut errors = Vec::new();
    if schema.is_none() && !values.is_empty() {
        errors.push("plugin does not declare userConfig".into());
    }
    if let Some(schema) = schema {
        for key in values.keys() {
            if !schema.contains_key(key) {
                errors.push(format!("unknown key `{key}`"));
            }
        }
        for (key, option) in schema {
            let value = values
                .get(key)
                .or(option.default.as_ref())
                .or_else(|| manifest.settings.as_ref().and_then(|settings| settings.get(key)));
            if require_complete && option.required && value.is_none() {
                errors.push(format!("required key `{key}` is missing"));
                continue;
            }
            if let Some(value) = value {
                validate_option(key, option, value, true, &mut errors);
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else if require_complete && errors.iter().any(|error| error.contains("is missing")) {
        Err(PluginError::UserConfigRequired { id: id.clone() })
    } else {
        Err(PluginError::UserConfigValidation { errors })
    }
}

fn validate_option(
    key: &str,
    option: &UserConfigOption,
    value: &Value,
    validate_paths: bool,
    errors: &mut Vec<String>,
) {
    let type_valid = match option.type_ {
        ConfigOptionType::String | ConfigOptionType::Directory | ConfigOptionType::File => {
            value.is_string()
        }
        ConfigOptionType::Number => value.is_number(),
        ConfigOptionType::Boolean => value.is_boolean(),
    };
    if !type_valid {
        errors.push(format!("`{key}` has the wrong type"));
        return;
    }
    if let Some(number) = value.as_f64() {
        if option.min.is_some_and(|min| number < min) {
            errors.push(format!("`{key}` is below its minimum"));
        }
        if option.max.is_some_and(|max| number > max) {
            errors.push(format!("`{key}` exceeds its maximum"));
        }
    }
    if validate_paths && let Some(path) = value.as_str() {
        match option.type_ {
            ConfigOptionType::Directory if !Path::new(path).is_dir() => {
                errors.push(format!("`{key}` is not an existing directory"));
            }
            ConfigOptionType::File if !Path::new(path).is_file() => {
                errors.push(format!("`{key}` is not an existing file"));
            }
            _ => {}
        }
    }
}

pub(crate) fn validate_manifest_config(manifest: &PluginManifest) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(schema) = &manifest.user_config {
        for (key, option) in schema {
            if key.trim().is_empty() {
                errors.push("userConfig keys must not be empty".into());
            }
            if option.title.trim().is_empty() {
                errors.push(format!("userConfig.{key}.title must not be empty"));
            }
            if option.description.trim().is_empty() {
                errors.push(format!("userConfig.{key}.description must not be empty"));
            }
            if option.min.is_some_and(|value| !value.is_finite())
                || option.max.is_some_and(|value| !value.is_finite())
            {
                errors.push(format!("userConfig.{key} bounds must be finite"));
            }
            if option.min.zip(option.max).is_some_and(|(minimum, maximum)| minimum > maximum) {
                errors.push(format!("userConfig.{key}.min must not exceed max"));
            }
            if !matches!(option.type_, ConfigOptionType::Number)
                && (option.min.is_some() || option.max.is_some())
            {
                errors.push(format!("userConfig.{key} bounds require type number"));
            }
            if let Some(default) = &option.default {
                validate_option(key, option, default, false, &mut errors);
            }
        }
    }
    let mut normalized_keys = HashMap::<String, String>::new();
    let keys = manifest
        .settings
        .iter()
        .flat_map(|settings| settings.keys())
        .chain(manifest.user_config.iter().flat_map(|schema| schema.keys()));
    for key in keys {
        let normalized = normalize_env_key(key);
        if normalized.is_empty() {
            errors.push(format!("configuration key `{key}` has no usable environment name"));
        } else if normalized == "CONFIG" {
            errors.push(format!(
                "configuration key `{key}` conflicts with reserved TELOS_PLUGIN_CONFIG"
            ));
        }
        if let Some(previous) = normalized_keys.insert(normalized.clone(), key.clone())
            && previous != *key
        {
            errors.push(format!(
                "configuration keys `{previous}` and `{key}` both map to TELOS_PLUGIN_{normalized}"
            ));
        }
    }
    errors
}

fn normalize_env_key(key: &str) -> String {
    key.chars()
        .map(
            |character| {
                if character.is_ascii_alphanumeric() { character.to_ascii_uppercase() } else { '_' }
            },
        )
        .collect()
}

fn scalar_env_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest() -> PluginManifest {
        serde_json::from_value(json!({
            "manifestVersion": 3,
            "name": "configured",
            "version": "1.0.0",
            "settings": {"mode": "safe"},
            "userConfig": {
                "token": {
                    "type": "string",
                    "title": "Token",
                    "description": "API token",
                    "required": true,
                    "sensitive": true
                },
                "limit": {
                    "type": "number",
                    "title": "Limit",
                    "description": "Limit",
                    "default": 3,
                    "min": 1,
                    "max": 5
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn validates_persists_and_redacts_plugin_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let id = PluginId::parse("configured@test").unwrap();
        let mut store = PluginConfigStore::new(&path);
        store.set(&id, &manifest(), HashMap::from([("token".into(), json!("secret"))])).unwrap();

        let mut loaded = PluginConfigStore::new(path);
        loaded.load().unwrap();
        let resolved = loaded.resolve(&id, &manifest()).unwrap();
        assert_eq!(resolved.values()["mode"], "safe");
        assert_eq!(resolved.values()["limit"], 3);
        assert_eq!(resolved.command_env()["TELOS_PLUGIN_TOKEN"], "secret");
        assert_eq!(resolved.redacted_values()["token"], "[REDACTED]");
    }

    #[test]
    fn rejects_unknown_and_out_of_range_values() {
        let id = PluginId::parse("configured@test").unwrap();
        let mut store = PluginConfigStore::new("unused.json");
        let error = store
            .set(
                &id,
                &manifest(),
                HashMap::from([("unknown".into(), json!(true)), ("limit".into(), json!(99))]),
            )
            .unwrap_err();
        assert!(matches!(error, PluginError::UserConfigValidation { .. }));
    }

    #[test]
    fn partial_updates_preserve_existing_sensitive_values() {
        let dir = tempfile::tempdir().unwrap();
        let id = PluginId::parse("configured@test").unwrap();
        let mut store = PluginConfigStore::new(dir.path().join("state.json"));
        store.set(&id, &manifest(), HashMap::from([("token".into(), json!("secret"))])).unwrap();

        store.set(&id, &manifest(), HashMap::from([("limit".into(), json!(4))])).unwrap();

        let resolved = store.resolve(&id, &manifest()).unwrap();
        assert_eq!(resolved.values()["token"], "secret");
        assert_eq!(resolved.values()["limit"], 4);
    }

    #[test]
    fn rejects_reserved_and_colliding_environment_keys() {
        let reserved: PluginManifest = serde_json::from_value(json!({
            "manifestVersion": 3,
            "name": "reserved",
            "version": "1.0.0",
            "settings": {"config": "bad"}
        }))
        .unwrap();
        assert!(
            validate_manifest_config(&reserved)
                .iter()
                .any(|error| error.contains("TELOS_PLUGIN_CONFIG"))
        );

        let colliding: PluginManifest = serde_json::from_value(json!({
            "manifestVersion": 3,
            "name": "colliding",
            "version": "1.0.0",
            "settings": {"foo-bar": 1, "foo_bar": 2}
        }))
        .unwrap();
        assert!(
            validate_manifest_config(&colliding)
                .iter()
                .any(|error| error.contains("both map to TELOS_PLUGIN_FOO_BAR"))
        );
    }
}
