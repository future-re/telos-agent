use std::path::Path;

pub(super) fn save_auto_mode(path: &Path, on: bool) -> anyhow::Result<()> {
    let mut config = load_toml_config(path);
    config
        .as_table_mut()
        .and_then(|table| table.insert("auto_mode".into(), toml::Value::Boolean(on)));
    write_toml_config(path, &config)
}

pub(super) fn save_deepseek_api_key(path: &Path, key: &str) -> anyhow::Result<()> {
    let mut config = load_toml_config(path);
    let table =
        config.as_table_mut().ok_or_else(|| anyhow::anyhow!("config root is not a table"))?;
    let env = table.entry("env").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let env_table =
        env.as_table_mut().ok_or_else(|| anyhow::anyhow!("config [env] is not a table"))?;
    env_table.insert("DEEPSEEK_API_KEY".into(), toml::Value::String(key.to_string()));
    write_toml_config(path, &config)
}

fn load_toml_config(path: &Path) -> toml::Value {
    let contents = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };
    if contents.is_empty() {
        toml::Value::Table(toml::Table::new())
    } else {
        toml::from_str(&contents).unwrap_or_else(|err| {
            tracing::warn!("failed to parse config at {}: {err}", path.display());
            toml::Value::Table(toml::Table::new())
        })
    }
}

fn write_toml_config(path: &Path, config: &toml::Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}
