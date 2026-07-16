use std::collections::BTreeMap;
use std::path::Path;

use crate::error::AgentError;
use crate::orchestration::subagent::builtins::builtin_agents;
use crate::orchestration::subagent::definition::{AgentDefinition, AgentSource};

/// Registry of available subagent definitions.
#[derive(Debug, Clone, Default)]
pub struct SubagentRegistry {
    agents: BTreeMap<String, AgentDefinition>,
}

impl SubagentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_builtin_agents() -> Self {
        let mut registry = Self::new();
        for agent in builtin_agents() {
            registry.register(agent);
        }
        registry
    }

    pub fn register(&mut self, definition: AgentDefinition) {
        self.agents.insert(definition.name.clone(), definition);
    }

    pub fn get(&self, name: &str) -> Option<&AgentDefinition> {
        self.agents.get(name)
    }

    pub fn definitions(&self) -> Vec<&AgentDefinition> {
        self.agents.values().collect()
    }

    pub fn render_listing(&self) -> String {
        self.agents
            .values()
            .map(|agent| {
                let tools = if agent.allowed_tools.is_empty() {
                    "All tools".to_string()
                } else {
                    agent.allowed_tools.join(", ")
                };
                format!("- {}: {} (Tools: {tools})", agent.name, agent.description)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn load_markdown_file(
        &mut self,
        path: impl AsRef<Path>,
        source: AgentSource,
    ) -> Result<(), AgentError> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(|err| {
            AgentError::Config(format!("failed to read agent file {}: {err}", path.display()))
        })?;
        let definition = AgentDefinition::from_markdown(&content, source)?;
        self.register(definition);
        Ok(())
    }

    pub fn load_markdown_dir(
        &mut self,
        dir: impl AsRef<Path>,
        source: AgentSource,
    ) -> Result<(), AgentError> {
        let dir = dir.as_ref();
        let entries = std::fs::read_dir(dir).map_err(|err| {
            AgentError::Config(format!("failed to read agent dir {}: {err}", dir.display()))
        })?;
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| {
                AgentError::Config(format!("failed to read agent dir entry: {err}"))
            })?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext == "md") {
                paths.push(path);
            }
        }
        paths.sort();
        for path in paths {
            self.load_markdown_file(&path, source.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::subagent::{AgentDefinition, AgentSource};

    #[test]
    fn later_agent_registration_overrides_earlier_definition() {
        let mut registry = SubagentRegistry::new();
        registry.register(AgentDefinition::new(
            "reviewer",
            "old",
            "old prompt",
            AgentSource::BuiltIn,
        ));
        registry.register(AgentDefinition::new(
            "reviewer",
            "new",
            "new prompt",
            AgentSource::Project { path: "agents/reviewer.md".into() },
        ));

        let definition = registry.get("reviewer").unwrap();
        assert_eq!(definition.description, "new");
        assert_eq!(registry.definitions().len(), 1);
    }

    #[test]
    fn builtins_include_core_agent_types() {
        let registry = SubagentRegistry::with_builtin_agents();
        assert!(registry.get("general-purpose").is_some());
        assert!(registry.get("Explore").is_some());
        assert!(registry.get("Plan").is_some());
        assert!(registry.get("Verification").is_some());
        assert!(registry.render_listing().contains("Explore"));
    }

    #[test]
    fn loads_agent_markdown_files_from_directory() {
        let dir = std::env::temp_dir().join("tiny_agent_subagent_registry_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("auditor.md"),
            r#"---
name: auditor
description: Audit changes.
tools: [Read]
---
You audit changes.
"#,
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let mut registry = SubagentRegistry::new();
        registry
            .load_markdown_dir(&dir, AgentSource::Project { path: dir.display().to_string() })
            .unwrap();

        let auditor = registry.get("auditor").unwrap();
        assert_eq!(auditor.system_prompt, "You audit changes.");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
