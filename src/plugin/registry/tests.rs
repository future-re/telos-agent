//! Tests for the plugin registry.

use crate::plugin::registry::lifecycle::PluginRegistry;
use crate::plugin::registry::types::{LoadedPlugin, PluginStatus};
use crate::plugin::{PluginError, PluginId, PluginManifest, PluginSource};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn make_plugin_dir(dir: &Path, name: &str, marketplace: &str) -> PathBuf {
    let plugin_dir = dir.join("installed").join(format!("{name}@{marketplace}"));
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let manifest = serde_json::json!({
        "name": name,
        "version": "1.0.0",
        "description": "A test plugin"
    });
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    plugin_dir
}

#[test]
fn register_and_get_plugin() {
    let tmp = TempDir::new().unwrap();
    let mut registry = PluginRegistry::new(tmp.path());

    let id = PluginId { name: "test".into(), marketplace: "test-mkt".into() };
    let plugin = LoadedPlugin {
        id: id.clone(),
        manifest: PluginManifest {
            name: "test".into(),
            ..serde_json::from_value(serde_json::json!({"name": "test"})).unwrap()
        },
        path: tmp.path().to_path_buf(),
        source: PluginSource::Local { path: tmp.path().to_path_buf() },
        enabled: false,
        is_builtin: false,
        resolved_tools: vec![],
        resolved_skills: vec![],
        resolved_agents: vec![],
        resolved_prompt_sections: vec![],
        resolved_output_styles: vec![],
    };

    registry.register(plugin);
    assert!(registry.get(&id).is_some());
    assert_eq!(registry.len(), 1);
}

#[test]
fn enable_disable_lifecycle() {
    let tmp = TempDir::new().unwrap();
    let mut registry = PluginRegistry::new(tmp.path());
    let id = PluginId { name: "t".into(), marketplace: "m".into() };
    let plugin = LoadedPlugin {
        id: id.clone(),
        manifest: serde_json::from_value(serde_json::json!({"name": "t"})).unwrap(),
        path: tmp.path().to_path_buf(),
        source: PluginSource::Local { path: tmp.path().to_path_buf() },
        enabled: false,
        is_builtin: false,
        resolved_tools: vec![],
        resolved_skills: vec![],
        resolved_agents: vec![],
        resolved_prompt_sections: vec![],
        resolved_output_styles: vec![],
    };

    registry.register(plugin);
    assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Disabled);

    registry.enable(&id).unwrap();
    assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Enabled);

    // Idempotent
    registry.enable(&id).unwrap();
    assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Enabled);

    registry.disable(&id).unwrap();
    assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Disabled);

    // Idempotent
    registry.disable(&id).unwrap();
    assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Disabled);
}

#[test]
fn enable_nonexistent_returns_error() {
    let tmp = TempDir::new().unwrap();
    let mut registry = PluginRegistry::new(tmp.path());
    let id = PluginId { name: "nope".into(), marketplace: "nope".into() };
    let result = registry.enable(&id);
    assert!(result.is_err());
}

#[test]
fn discover_installed_finds_plugins() {
    let tmp = TempDir::new().unwrap();
    make_plugin_dir(tmp.path(), "my-plugin", "community");
    make_plugin_dir(tmp.path(), "other", "telos-official");

    let mut registry = PluginRegistry::new(tmp.path());
    let discovered = registry.discover_installed().unwrap();
    assert_eq!(discovered.len(), 2);
    assert_eq!(registry.len(), 2);
}

#[test]
fn discover_skips_non_plugin_dirs() {
    let tmp = TempDir::new().unwrap();
    let installed = tmp.path().join("installed");
    std::fs::create_dir_all(&installed).unwrap();
    // Empty directory — no plugin.json
    std::fs::create_dir_all(installed.join("not-a-plugin")).unwrap();

    let mut registry = PluginRegistry::new(tmp.path());
    let discovered = registry.discover_installed().unwrap();
    assert!(discovered.is_empty());
}

#[test]
fn save_and_load_state() {
    let tmp = TempDir::new().unwrap();
    make_plugin_dir(tmp.path(), "p1", "mkt");
    make_plugin_dir(tmp.path(), "p2", "mkt");

    let mut registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();

    // Enable p1, keep p2 disabled
    let id1 = PluginId::parse("p1@mkt").unwrap();
    let id2 = PluginId::parse("p2@mkt").unwrap();
    registry.enable(&id1).unwrap();
    registry.save_state().unwrap();

    // Create a fresh registry and load state
    let mut registry2 = PluginRegistry::new(tmp.path());
    registry2.discover_installed().unwrap();
    registry2.load_state().unwrap();

    assert_eq!(registry2.get(&id1).unwrap().status, PluginStatus::Enabled);
    assert_eq!(registry2.get(&id2).unwrap().status, PluginStatus::Disabled);
}

#[test]
fn list_enabled_and_disabled() {
    let tmp = TempDir::new().unwrap();
    make_plugin_dir(tmp.path(), "a", "m");
    make_plugin_dir(tmp.path(), "b", "m");

    let mut registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();
    let id_a = PluginId::parse("a@m").unwrap();
    registry.enable(&id_a).unwrap();

    assert_eq!(registry.list_enabled().len(), 1);
    assert_eq!(registry.list_disabled().len(), 1);
    assert_eq!(registry.list_all().len(), 2);
}

#[test]
fn mark_degraded_and_error() {
    let tmp = TempDir::new().unwrap();
    make_plugin_dir(tmp.path(), "d", "m");

    let mut registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();
    let id = PluginId::parse("d@m").unwrap();

    registry.mark_degraded(&id, vec![PluginError::Other("partial load".into())]);
    assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Degraded);

    registry.mark_error(&id, PluginError::Other("total failure".into()));
    assert_eq!(registry.get(&id).unwrap().status, PluginStatus::Error);
}

#[test]
fn apply_registers_plugin_tools_with_namespace() {
    let tmp = TempDir::new().unwrap();
    let plugin_dir = tmp.path().join("installed").join("test-plugin@mkt");
    std::fs::create_dir_all(plugin_dir.join("tools")).unwrap();

    // Write plugin.json
    let manifest = serde_json::json!({
        "name": "test-plugin",
        "version": "1.0.0",
        "tools": ["./tools/hello.json"]
    });
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Write a tool spec
    let tool_spec = serde_json::json!({
        "name": "hello",
        "description": "Says hello",
        "inputSchema": {"type": "object"},
        "command": "echo",
        "permission": "allow"
    });
    std::fs::write(
        plugin_dir.join("tools").join("hello.json"),
        serde_json::to_string_pretty(&tool_spec).unwrap(),
    )
    .unwrap();

    let mut registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();
    let id = PluginId::parse("test-plugin@mkt").unwrap();
    registry.enable(&id).unwrap();

    let mut tools = crate::tool::ToolRegistry::new();
    let mut hooks = crate::hooks::HookRegistry::new();
    let mut skills = crate::skills::SkillRegistry::new();
    let mut mcp_config = crate::mcp::McpManager::new(std::collections::HashMap::new());
    let mut prompt = crate::prompt::PromptAssembly::new();

    let result = registry.apply(&mut tools, &mut hooks, &mut skills, &mut mcp_config, &mut prompt);
    assert!(result.is_ok());

    // Tool should be registered with namespace
    let tool = tools.get("plugin__test-plugin__hello");
    assert!(tool.is_ok(), "plugin tool should be registered with namespace prefix");
}
