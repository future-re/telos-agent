//! Subagent module — in-process nested agents and the Fork concurrent-execution engine.

pub mod builtins;
pub mod definition;
pub mod fork;
pub mod registry;
mod tool;
pub mod worktree;

use std::sync::Arc;

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::model::provider::ModelProvider;
use crate::tools::api::ToolRegistry;

pub use definition::{AgentDefinition, AgentIsolation, AgentSource};
pub use fork::{ForkExecution, ForkLens, ForkResult, ForkShared, Synapse};
pub use registry::SubagentRegistry;
pub use tool::SubagentTool;
pub use worktree::{WorktreeInfo, create_subagent_worktree};

/// Register the in-process subagent tool with built-in and plugin-provided agent definitions.
pub fn register_subagent_tool(
    tools: &mut ToolRegistry,
    provider: Arc<dyn ModelProvider + Send + Sync>,
    config: &AgentConfig,
) -> Result<(), AgentError> {
    let mut subagents = SubagentRegistry::with_builtin_agents();
    if let Some(plugin_registry) = &config.plugin_registry
        && let Err(errors) = plugin_registry.apply_subagents(&mut subagents)
    {
        for error in errors {
            tracing::warn!(error = %error, "failed to load plugin subagent component");
        }
    }

    tools.register(SubagentTool::with_registry(provider, tools.clone(), config.clone(), subagents));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::plugin::{PluginId, PluginRegistry};
    use crate::model::mock::MockProvider;
    use crate::tools::api::ToolContext;
    use serde_json::json;

    #[tokio::test]
    async fn register_subagent_tool_loads_plugin_subagents() {
        let temp = tempfile::tempdir().unwrap();
        let plugin_dir = temp.path().join("installed").join("agent-plugin@mkt");
        std::fs::create_dir_all(plugin_dir.join("agents")).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_string_pretty(&json!({
                "name": "agent-plugin",
                "version": "1.0.0",
                "agents": ["./agents/auditor.md"]
            }))
            .unwrap(),
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

        let mut plugins = PluginRegistry::new(temp.path());
        plugins.discover_installed().unwrap();
        plugins.enable(&PluginId::parse("agent-plugin@mkt").unwrap()).unwrap();

        let config =
            AgentConfig { plugin_registry: Some(Arc::new(plugins)), ..AgentConfig::default() };
        let mut tools = ToolRegistry::new();
        register_subagent_tool(&mut tools, Arc::new(MockProvider::new(vec![])), &config).unwrap();

        let subagent = tools.get("subagent").unwrap();
        subagent
            .validate(
                &json!({
                    "prompt": "audit",
                    "subagent_type": "agent-plugin:auditor"
                }),
                &ToolContext::dummy(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn register_subagent_tool_keeps_valid_plugin_subagents_when_one_fails() {
        let temp = tempfile::tempdir().unwrap();
        let plugin_dir = temp.path().join("installed").join("agent-plugin@mkt");
        std::fs::create_dir_all(plugin_dir.join("agents")).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_string_pretty(&json!({
                "name": "agent-plugin",
                "version": "1.0.0",
                "agents": ["./agents/auditor.md", "./agents/bad.md"]
            }))
            .unwrap(),
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
        std::fs::write(
            plugin_dir.join("agents").join("bad.md"),
            r#"---
name: bad
tools: [Read]
---
Missing required description.
"#,
        )
        .unwrap();

        let mut plugins = PluginRegistry::new(temp.path());
        plugins.discover_installed().unwrap();
        plugins.enable(&PluginId::parse("agent-plugin@mkt").unwrap()).unwrap();

        let config =
            AgentConfig { plugin_registry: Some(Arc::new(plugins)), ..AgentConfig::default() };
        let mut tools = ToolRegistry::new();
        register_subagent_tool(&mut tools, Arc::new(MockProvider::new(vec![])), &config).unwrap();

        let subagent = tools.get("subagent").unwrap();
        subagent
            .validate(
                &json!({
                    "prompt": "audit",
                    "subagent_type": "agent-plugin:auditor"
                }),
                &ToolContext::dummy(),
            )
            .await
            .unwrap();
    }
}
