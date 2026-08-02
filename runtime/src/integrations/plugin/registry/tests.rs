//! Tests for the plugin registry.

use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::integrations::plugin::registry::types::{LoadedPlugin, PluginStatus};
use crate::integrations::plugin::{PluginError, PluginId, PluginManifest, PluginSource};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn make_plugin_dir(dir: &Path, name: &str, marketplace: &str) -> PathBuf {
    let plugin_dir = dir.join("installed").join(format!("{name}@{marketplace}"));
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let manifest = serde_json::json!({
        "manifestVersion": 2,
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
    let registry = PluginRegistry::new(tmp.path());

    let id = PluginId { name: "test".into(), marketplace: "test-mkt".into() };
    let plugin = LoadedPlugin {
        id: id.clone(),
        manifest: PluginManifest {
            name: "test".into(),
            ..serde_json::from_value(serde_json::json!({
                "manifestVersion": 2,
                "name": "test",
                "version": "1.0.0"
            }))
            .unwrap()
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
    let registry = PluginRegistry::new(tmp.path());
    let id = PluginId { name: "t".into(), marketplace: "m".into() };
    let plugin = LoadedPlugin {
        id: id.clone(),
        manifest: serde_json::from_value(serde_json::json!({
            "manifestVersion": 2,
            "name": "t",
            "version": "1.0.0"
        }))
        .unwrap(),
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
    let registry = PluginRegistry::new(tmp.path());
    let id = PluginId { name: "nope".into(), marketplace: "nope".into() };
    let result = registry.enable(&id);
    assert!(result.is_err());
}

#[test]
fn enable_requires_complete_plugin_configuration() {
    let tmp = TempDir::new().unwrap();
    let plugin_dir = tmp.path().join("installed/configured@mkt");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "manifestVersion": 2,
            "name": "configured",
            "version": "1.0.0",
            "userConfig": {
                "token": {
                    "type": "string",
                    "title": "Token",
                    "description": "API token",
                    "required": true,
                    "sensitive": true
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();
    let id = PluginId::parse("configured@mkt").unwrap();

    assert!(matches!(registry.enable(&id), Err(PluginError::UserConfigRequired { .. })));
    registry
        .set_config(&id, HashMap::from([("token".into(), serde_json::json!("secret"))]))
        .unwrap();
    registry.enable(&id).unwrap();
}

#[test]
fn discover_installed_finds_plugins() {
    let tmp = TempDir::new().unwrap();
    make_plugin_dir(tmp.path(), "my-plugin", "community");
    make_plugin_dir(tmp.path(), "other", "telos-official");

    let registry = PluginRegistry::new(tmp.path());
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

    let registry = PluginRegistry::new(tmp.path());
    let discovered = registry.discover_installed().unwrap();
    assert!(discovered.is_empty());
}

#[test]
fn discovery_rejects_removed_hooks_field() {
    let tmp = TempDir::new().unwrap();
    let plugin_dir = tmp.path().join("installed").join("legacy@mkt");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("plugin.json"), r#"{"name":"legacy","hooks":{"Stop":[]}}"#)
        .unwrap();

    let registry = PluginRegistry::new(tmp.path());
    assert!(registry.discover_installed().unwrap().is_empty());
    assert!(registry.is_empty());
}

#[test]
fn save_and_load_state() {
    let tmp = TempDir::new().unwrap();
    make_plugin_dir(tmp.path(), "p1", "mkt");
    make_plugin_dir(tmp.path(), "p2", "mkt");

    let registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();

    // Enable p1, keep p2 disabled
    let id1 = PluginId::parse("p1@mkt").unwrap();
    let id2 = PluginId::parse("p2@mkt").unwrap();
    registry.enable(&id1).unwrap();
    registry.mark_degraded(&id1, vec![PluginError::Other("component failed".into())]);
    registry.save_state().unwrap();

    // Create a fresh registry and load state
    let registry2 = PluginRegistry::new(tmp.path());
    registry2.discover_installed().unwrap();
    registry2.load_state().unwrap();

    assert_eq!(registry2.get(&id1).unwrap().status, PluginStatus::Degraded);
    assert_eq!(registry2.get(&id1).unwrap().load_errors[0].to_string(), "component failed");
    assert_eq!(registry2.get(&id2).unwrap().status, PluginStatus::Disabled);
}

#[test]
fn list_enabled_and_disabled() {
    let tmp = TempDir::new().unwrap();
    make_plugin_dir(tmp.path(), "a", "m");
    make_plugin_dir(tmp.path(), "b", "m");

    let registry = PluginRegistry::new(tmp.path());
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

    let registry = PluginRegistry::new(tmp.path());
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
        "manifestVersion": 2,
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

    let registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();
    let id = PluginId::parse("test-plugin@mkt").unwrap();
    registry.enable(&id).unwrap();

    let mut tools = crate::tools::api::ToolRegistry::new();
    let mut policies = crate::agent::policies::PolicyRegistry::new();
    let mut skills = crate::knowledge::skills::SkillRegistry::new();
    let mut mcp_config =
        crate::integrations::mcp::McpManager::new(std::collections::HashMap::new());
    let mut prompt = crate::agent::prompt::PromptAssembly::new();

    let result = registry.apply(
        &mut tools,
        &mut policies,
        &std::collections::HashMap::new(),
        &mut skills,
        &mut mcp_config,
        &mut prompt,
    );
    assert!(result.is_ok());

    // Tool should be registered with namespace
    let tool = tools.get("plugin__test-plugin__mkt__hello");
    assert!(tool.is_ok(), "plugin tool should be registered with namespace prefix");
}

#[cfg(unix)]
#[tokio::test]
async fn apply_registers_and_executes_command_policy() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().unwrap();
    let plugin_dir = tmp.path().join("installed").join("policy-plugin@mkt");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let script = plugin_dir.join("guard.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{\"decision\":\"continue\",\"feedback\":[\"from plugin\"]}'\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).unwrap();
    let manifest = serde_json::json!({
        "manifestVersion": 2,
        "name": "policy-plugin",
        "version": "1.0.0",
        "policies": {
            "sessionStart": [{
                "name": "session-guard",
                "command": "./guard.sh"
            }]
        }
    });
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();
    registry.enable(&PluginId::parse("policy-plugin@mkt").unwrap()).unwrap();
    let mut tools = crate::ToolRegistry::new();
    let mut policies = crate::PolicyRegistry::new();
    let mut skills = crate::SkillRegistry::new();
    let mut mcp = crate::McpManager::new(std::collections::HashMap::new());
    let mut prompt = crate::PromptAssembly::new();
    registry
        .apply(
            &mut tools,
            &mut policies,
            &std::collections::HashMap::new(),
            &mut skills,
            &mut mcp,
            &mut prompt,
        )
        .unwrap();

    let registered = policies.session_start(crate::SessionMode::Create);
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0].name(), "plugin::policy-plugin__mkt::session-guard");
    let outcome = registered[0]
        .evaluate(&crate::PolicyContext::SessionStart {
            session_id: "session-1".into(),
            mode: crate::SessionMode::Create,
            message_count: 0,
        })
        .await
        .unwrap();
    assert_eq!(outcome.feedback, vec!["from plugin"]);
}

#[tokio::test]
async fn apply_activates_output_styles_settings_and_lsp_servers() {
    let tmp = TempDir::new().unwrap();
    let plugin_dir = tmp.path().join("installed").join("future-plugin@mkt");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("style.json"),
        r#"{"name":"concise","instructions":"Use ${CONFIG:theme} output."}"#,
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "manifestVersion": 2,
            "name": "future-plugin",
            "version": "1.0.0",
            "outputStyles": ["style.json"],
            "settings": {"theme": "dark"},
            "lspServers": {
                "rust": {
                    "command": "rust-analyzer",
                    "extensionToLanguage": {".rs": "rust"}
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let registry = PluginRegistry::new(tmp.path());
    registry.discover_installed().unwrap();
    registry.enable(&PluginId::parse("future-plugin@mkt").unwrap()).unwrap();
    let mut tools = crate::ToolRegistry::new();
    let mut policies = crate::PolicyRegistry::new();
    let mut skills = crate::SkillRegistry::new();
    let mut mcp = crate::McpManager::new(std::collections::HashMap::new());
    let mut prompt = crate::PromptAssembly::new();

    registry
        .apply(
            &mut tools,
            &mut policies,
            &std::collections::HashMap::new(),
            &mut skills,
            &mut mcp,
            &mut prompt,
        )
        .unwrap();

    assert!(tools.get("plugin__future-plugin__mkt__lsp__rust").is_ok());
    assert!(prompt.build().await.contains("Use dark output."));
}

#[test]
fn apply_subagents_registers_plugin_agents_with_namespace() {
    let tmp = TempDir::new().unwrap();
    let plugin_dir = tmp.path().join("installed").join("agent-plugin@mkt");
    std::fs::create_dir_all(plugin_dir.join("agents")).unwrap();

    let manifest = serde_json::json!({
        "manifestVersion": 2,
        "name": "agent-plugin",
        "version": "1.0.0",
        "agents": ["./agents/auditor.md"]
    });
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("agents").join("auditor.md"),
        r#"---
name: auditor
description: Audit plugin-provided behavior.
tools: [Read]
---
You audit plugin behavior.
"#,
    )
    .unwrap();

    let plugins = PluginRegistry::new(tmp.path());
    plugins.discover_installed().unwrap();
    let id = PluginId::parse("agent-plugin@mkt").unwrap();
    plugins.enable(&id).unwrap();

    let mut subagents = crate::orchestration::subagent::SubagentRegistry::new();
    plugins.apply_subagents(&mut subagents).unwrap();

    let agent = subagents.get("agent-plugin@mkt:auditor").unwrap();
    assert_eq!(agent.description, "Audit plugin-provided behavior.");
    assert_eq!(agent.system_prompt, "You audit plugin behavior.");
}

#[test]
fn apply_keeps_same_named_plugins_from_different_marketplaces_distinct() {
    let tmp = TempDir::new().unwrap();
    for marketplace in ["official", "community"] {
        let plugin_dir = tmp.path().join("installed").join(format!("formatter@{marketplace}"));
        std::fs::create_dir_all(plugin_dir.join("tools")).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "manifestVersion": 2,
                "name": "formatter",
                "version": "1.0.0",
                "tools": ["./tools/format.json"]
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            plugin_dir.join("tools/format.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "name": "format",
                "description": "Format a file",
                "inputSchema": {"type": "object"},
                "command": "echo",
                "permission": "allow"
            }))
            .unwrap(),
        )
        .unwrap();
    }

    let plugins = PluginRegistry::new(tmp.path());
    plugins.discover_installed().unwrap();
    plugins.enable(&PluginId::parse("formatter@official").unwrap()).unwrap();
    plugins.enable(&PluginId::parse("formatter@community").unwrap()).unwrap();
    let mut tools = crate::ToolRegistry::new();
    let mut policies = crate::PolicyRegistry::new();
    let mut skills = crate::SkillRegistry::new();
    let mut mcp = crate::McpManager::new(std::collections::HashMap::new());
    let mut prompt = crate::PromptAssembly::new();

    plugins
        .apply(
            &mut tools,
            &mut policies,
            &std::collections::HashMap::new(),
            &mut skills,
            &mut mcp,
            &mut prompt,
        )
        .unwrap();

    assert!(tools.get("plugin__formatter__official__format").is_ok());
    assert!(tools.get("plugin__formatter__community__format").is_ok());
}

#[test]
fn component_namespace_encoding_does_not_collapse_dots_and_underscores() {
    assert_ne!(
        super::apply::normalize_component_name("formatter.one"),
        super::apply::normalize_component_name("formatter_one")
    );
}

#[test]
fn transactional_local_install_resolves_dependencies_upgrades_and_uninstalls() {
    let tmp = TempDir::new().unwrap();
    let sources = tmp.path().join("sources");
    for (name, dependencies) in [
        ("base", serde_json::json!([])),
        ("app", serde_json::json!([{"name": "base", "version": "^1"}])),
    ] {
        let root = sources.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "manifestVersion": 2,
                "name": name,
                "version": "1.0.0",
                "dependencies": dependencies
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let build_marketplace = |app_version: semver::Version| {
        let entries = ["base", "app"]
            .into_iter()
            .map(|name| crate::MarketplaceEntry {
                name: name.into(),
                description: None,
                version: if name == "app" {
                    app_version.clone()
                } else {
                    semver::Version::new(1, 0, 0)
                },
                source: PluginSource::Local { path: sources.join(name) },
                category: None,
                tags: Vec::new(),
                strict: true,
                manifest_override: None,
            })
            .collect();
        let mut marketplaces = crate::MarketplaceRegistry::new(tmp.path().join("cache"));
        marketplaces
            .add(crate::MarketplaceSource::Inline { name: "test".into(), plugins: entries })
            .unwrap();
        marketplaces
    };
    let mut marketplaces = build_marketplace(semver::Version::new(1, 0, 0));
    let registry = PluginRegistry::new(tmp.path().join("plugins"));
    let app = PluginId::parse("app@test").unwrap();
    let base = PluginId::parse("base@test").unwrap();

    registry.install(&marketplaces, &app).unwrap();
    assert!(registry.is_installed(&app));
    assert!(registry.is_installed(&base));
    assert!(matches!(registry.uninstall(&base), Err(PluginError::DependencyRequiredBy { .. })));

    registry.enable(&base).unwrap();
    registry.enable(&app).unwrap();

    let app_source = sources.join("app").join("plugin.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&app_source).unwrap()).unwrap();
    manifest["version"] = serde_json::json!("2.0.0");
    std::fs::write(&app_source, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    marketplaces = build_marketplace(semver::Version::new(2, 0, 0));
    registry.upgrade(&marketplaces, &app).unwrap();
    assert_eq!(registry.get(&app).unwrap().plugin.manifest.version, semver::Version::new(2, 0, 0));
    assert_eq!(registry.get(&app).unwrap().status, PluginStatus::Enabled);

    manifest["version"] = serde_json::json!("3.0.0");
    manifest["dependencies"] = serde_json::json!([{"name": "missing", "version": "^1"}]);
    std::fs::write(&app_source, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    marketplaces = build_marketplace(semver::Version::new(3, 0, 0));
    assert!(registry.upgrade(&marketplaces, &app).is_err());
    assert_eq!(registry.get(&app).unwrap().plugin.manifest.version, semver::Version::new(2, 0, 0));
    assert_eq!(registry.get(&app).unwrap().status, PluginStatus::Enabled);

    registry.disable(&app).unwrap();
    registry.uninstall(&app).unwrap();
    registry.disable(&base).unwrap();
    registry.uninstall(&base).unwrap();
    assert!(registry.is_empty());
}

#[test]
fn install_rejects_catalog_and_manifest_version_mismatch() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("plugin.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "manifestVersion": 2,
            "name": "mismatch",
            "version": "2.0.0"
        }))
        .unwrap(),
    )
    .unwrap();
    let entry = crate::MarketplaceEntry {
        name: "mismatch".into(),
        description: None,
        version: semver::Version::new(1, 0, 0),
        source: PluginSource::Local { path: source },
        category: None,
        tags: Vec::new(),
        strict: true,
        manifest_override: None,
    };
    let mut marketplaces = crate::MarketplaceRegistry::new(tmp.path().join("cache"));
    marketplaces
        .add(crate::MarketplaceSource::Inline { name: "test".into(), plugins: vec![entry] })
        .unwrap();
    let registry = PluginRegistry::new(tmp.path().join("plugins"));

    let error =
        registry.install(&marketplaces, &PluginId::parse("mismatch@test").unwrap()).unwrap_err();
    assert!(matches!(error, PluginError::VersionMismatch { .. }));
    assert!(registry.is_empty());
}

#[test]
fn install_rejects_dependency_version_without_a_global_solution() {
    let tmp = TempDir::new().unwrap();
    let sources = tmp.path().join("sources");
    for (name, dependencies) in [
        ("base", serde_json::json!([])),
        ("app", serde_json::json!([{"name": "base", "version": "^2"}])),
    ] {
        let source = sources.join(name);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("plugin.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "manifestVersion": 2,
                "name": name,
                "version": "1.0.0",
                "dependencies": dependencies
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let entries = ["base", "app"]
        .into_iter()
        .map(|name| crate::MarketplaceEntry {
            name: name.into(),
            description: None,
            version: semver::Version::new(1, 0, 0),
            source: PluginSource::Local { path: sources.join(name) },
            category: None,
            tags: Vec::new(),
            strict: true,
            manifest_override: None,
        })
        .collect();
    let mut marketplaces = crate::MarketplaceRegistry::new(tmp.path().join("cache"));
    marketplaces
        .add(crate::MarketplaceSource::Inline { name: "test".into(), plugins: entries })
        .unwrap();
    let registry = PluginRegistry::new(tmp.path().join("plugins"));

    let error = registry.install(&marketplaces, &PluginId::parse("app@test").unwrap()).unwrap_err();
    assert!(matches!(error, PluginError::DependencyVersionConflict { .. }));
    assert!(registry.is_empty());
}

#[test]
fn upgrade_atomically_moves_shared_dependency_to_the_solved_version() {
    let tmp = TempDir::new().unwrap();
    let sources = tmp.path().join("sources");
    let write_sources = |version: &str, requirement: &str| {
        for name in ["base", "app"] {
            let source = sources.join(name);
            std::fs::create_dir_all(&source).unwrap();
            let dependencies = if name == "app" {
                serde_json::json!([{"name": "base", "version": requirement}])
            } else {
                serde_json::json!([])
            };
            std::fs::write(
                source.join("plugin.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "manifestVersion": 2,
                    "name": name,
                    "version": version,
                    "dependencies": dependencies
                }))
                .unwrap(),
            )
            .unwrap();
        }
    };
    let marketplace = |version: semver::Version| {
        let entries = ["base", "app"]
            .into_iter()
            .map(|name| crate::MarketplaceEntry {
                name: name.into(),
                description: None,
                version: version.clone(),
                source: PluginSource::Local { path: sources.join(name) },
                category: None,
                tags: Vec::new(),
                strict: true,
                manifest_override: None,
            })
            .collect();
        let mut marketplaces = crate::MarketplaceRegistry::new(tmp.path().join("cache"));
        marketplaces
            .add(crate::MarketplaceSource::Inline { name: "test".into(), plugins: entries })
            .unwrap();
        marketplaces
    };
    let registry = PluginRegistry::new(tmp.path().join("plugins"));
    let app = PluginId::parse("app@test").unwrap();
    let base = PluginId::parse("base@test").unwrap();
    write_sources("1.0.0", "^1");
    registry.install(&marketplace(semver::Version::new(1, 0, 0)), &app).unwrap();

    write_sources("2.0.0", "^2");
    let updated = marketplace(semver::Version::new(2, 0, 0));
    super::install::fail_commit_at(1);
    assert!(registry.upgrade(&updated, &app).is_err());
    assert_eq!(registry.get(&app).unwrap().plugin.manifest.version, semver::Version::new(1, 0, 0));
    assert_eq!(registry.get(&base).unwrap().plugin.manifest.version, semver::Version::new(1, 0, 0));

    registry.upgrade(&updated, &app).unwrap();

    assert_eq!(registry.get(&app).unwrap().plugin.manifest.version, semver::Version::new(2, 0, 0));
    assert_eq!(registry.get(&base).unwrap().plugin.manifest.version, semver::Version::new(2, 0, 0));
}

#[test]
fn upgrade_rejects_existing_configuration_incompatible_with_new_schema() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    std::fs::create_dir_all(&source).unwrap();
    let write_manifest = |version: &str, user_config: serde_json::Value| {
        std::fs::write(
            source.join("plugin.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "manifestVersion": 2,
                "name": "configured",
                "version": version,
                "userConfig": user_config
            }))
            .unwrap(),
        )
        .unwrap();
    };
    let schema = |key: &str| {
        serde_json::json!({
            (key): {
                "type": "string",
                "title": key,
                "description": key
            }
        })
    };
    let marketplace = |version: semver::Version| {
        let entry = crate::MarketplaceEntry {
            name: "configured".into(),
            description: None,
            version,
            source: PluginSource::Local { path: source.clone() },
            category: None,
            tags: Vec::new(),
            strict: true,
            manifest_override: None,
        };
        let mut marketplaces = crate::MarketplaceRegistry::new(tmp.path().join("cache"));
        marketplaces
            .add(crate::MarketplaceSource::Inline { name: "test".into(), plugins: vec![entry] })
            .unwrap();
        marketplaces
    };
    write_manifest("1.0.0", schema("mode"));
    let registry = PluginRegistry::new(tmp.path().join("plugins"));
    let id = PluginId::parse("configured@test").unwrap();
    registry.install(&marketplace(semver::Version::new(1, 0, 0)), &id).unwrap();
    registry
        .set_config(&id, HashMap::from([("mode".into(), serde_json::json!("strict"))]))
        .unwrap();

    write_manifest("2.0.0", schema("profile"));
    let error = registry.upgrade(&marketplace(semver::Version::new(2, 0, 0)), &id).unwrap_err();

    assert!(matches!(error, PluginError::UserConfigValidation { .. }));
    assert_eq!(registry.get(&id).unwrap().plugin.manifest.version, semver::Version::new(1, 0, 0));
}

#[test]
fn install_rejects_dependency_cycles_without_committing_plugins() {
    let tmp = TempDir::new().unwrap();
    let sources = tmp.path().join("cycle-sources");
    for (name, dependency) in [("a", "b"), ("b", "a")] {
        let root = sources.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "manifestVersion": 2,
                "name": name,
                "version": "1.0.0",
                "dependencies": [{"name": dependency, "version": "^1"}]
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let entries = ["a", "b"]
        .into_iter()
        .map(|name| crate::MarketplaceEntry {
            name: name.into(),
            description: None,
            version: semver::Version::new(1, 0, 0),
            source: PluginSource::Local { path: sources.join(name) },
            category: None,
            tags: Vec::new(),
            strict: true,
            manifest_override: None,
        })
        .collect();
    let mut marketplaces = crate::MarketplaceRegistry::new(tmp.path().join("cache"));
    marketplaces
        .add(crate::MarketplaceSource::Inline { name: "cycles".into(), plugins: entries })
        .unwrap();
    let registry = PluginRegistry::new(tmp.path().join("plugins"));

    let error = registry.install(&marketplaces, &PluginId::parse("a@cycles").unwrap()).unwrap_err();

    assert!(matches!(error, PluginError::CircularDependency { .. }));
    assert!(registry.is_empty());
    assert!(
        registry
            .installed_dir()
            .read_dir()
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(true)
    );
}

#[test]
fn non_strict_marketplace_entry_can_synthesize_manifest() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("manifestless");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("instructions.md"), "Plugin instructions").unwrap();
    let entry = crate::MarketplaceEntry {
        name: "synthesized".into(),
        description: Some("Entry-owned manifest".into()),
        version: semver::Version::new(1, 0, 0),
        source: PluginSource::Local { path: source },
        category: None,
        tags: Vec::new(),
        strict: false,
        manifest_override: Some(serde_json::json!({
            "promptSections": ["./instructions.md"]
        })),
    };
    let mut marketplaces = crate::MarketplaceRegistry::new(tmp.path().join("cache"));
    marketplaces
        .add(crate::MarketplaceSource::Inline { name: "entry-only".into(), plugins: vec![entry] })
        .unwrap();
    let registry = PluginRegistry::new(tmp.path().join("plugins"));
    let id = PluginId::parse("synthesized@entry-only").unwrap();

    registry.install(&marketplaces, &id).unwrap();

    let installed = registry.get(&id).unwrap();
    assert_eq!(installed.plugin.manifest.version, semver::Version::new(1, 0, 0));
    assert_eq!(installed.plugin.resolved_prompt_sections.len(), 1);
}
