use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::FileConfig;

/// Load config from an explicit file path. Returns `Ok(None)` if the file
/// does not exist.
pub fn load_config_file(path: &Path) -> Result<Option<FileConfig>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;
    let cfg: FileConfig = toml::from_str(&contents)
        .with_context(|| format!("failed to parse config file: {}", path.display()))?;
    Ok(Some(cfg))
}

/// Load user config from the given path, or from the default
/// `~/.config/telos/config.toml` if `config_path` is `None`.
pub fn load_user_config(config_path: Option<&Path>) -> Result<Option<FileConfig>> {
    let path = match config_path {
        Some(p) => p.to_path_buf(),
        None => {
            let base = dirs::config_dir().context("could not determine user config directory")?;
            base.join("telos").join("config.toml")
        }
    };
    load_config_file(&path)
}

/// Load config from a project `.telos.toml` located under `dir`.
pub fn load_project_config(dir: &Path) -> Result<Option<FileConfig>> {
    let path = dir.join(".telos.toml");
    load_config_file(&path)
}

pub fn default_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
