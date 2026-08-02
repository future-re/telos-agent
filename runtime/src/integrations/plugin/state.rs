//! Atomic persistence for the single plugin management state file.

use std::path::Path;

use serde_json::{Map, Value};

use crate::integrations::plugin::PluginError;

const STATE_VERSION: u64 = 1;

pub(crate) fn read(path: &Path) -> Result<Value, PluginError> {
    if !path.exists() {
        return Ok(empty());
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let version = value.get("version").and_then(Value::as_u64).unwrap_or(0);
    if version != STATE_VERSION {
        return Err(PluginError::Other(format!(
            "unsupported plugin state version {version}; expected {STATE_VERSION}"
        )));
    }
    Ok(value)
}

pub(crate) fn write_section(path: &Path, name: &str, section: Value) -> Result<(), PluginError> {
    let mut state = read(path)?;
    state
        .as_object_mut()
        .expect("plugin state root is initialized as an object")
        .insert(name.to_string(), section);
    write(path, &state)
}

fn empty() -> Value {
    let mut root = Map::new();
    root.insert("version".into(), Value::from(STATE_VERSION));
    Value::Object(root)
}

fn write(path: &Path, value: &Value) -> Result<(), PluginError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    restrict_permissions(&temporary)?;
    replace(&temporary, path)?;
    restrict_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), PluginError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), PluginError> {
    Ok(())
}

#[cfg(windows)]
fn replace(source: &Path, target: &Path) -> std::io::Result<()> {
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
fn replace(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(source, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_one_section_preserves_the_others() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.json");
        write_section(&path, "plugins", serde_json::json!({"a@test": "disabled"})).unwrap();
        write_section(&path, "config", serde_json::json!({"a@test": {"mode": "safe"}})).unwrap();

        let state = read(&path).unwrap();
        assert_eq!(state["plugins"]["a@test"], "disabled");
        assert_eq!(state["config"]["a@test"]["mode"], "safe");
    }
}
