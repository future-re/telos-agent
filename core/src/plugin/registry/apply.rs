//! Apply enabled plugin components to agent registries.

use crate::plugin::PluginError;
use crate::plugin::registry::lifecycle::PluginRegistry;
use crate::subagent::{AgentDefinition, AgentSource, SubagentRegistry};
use std::path::Path;

impl PluginRegistry {
    /// Apply all enabled plugins' components into the agent extension registries.
    ///
    /// # Namespacing
    /// Plugin tools are registered as `plugin__<plugin_name>__<tool_name>` to
    /// avoid conflicts with built-in tools.
    ///
    /// # Errors
    /// Returns a list of per-plugin errors. Plugins that fail component loading
    /// are marked Degraded; their successfully-loaded components remain active.
    pub fn apply(
        &self,
        tools: &mut crate::tool::ToolRegistry,
        _hooks: &mut crate::hooks::HookRegistry,
        skills: &mut crate::skills::SkillRegistry,
        _mcp: &mut crate::mcp::McpManager,
        prompt: &mut crate::prompt::PromptAssembly,
    ) -> Result<(), Vec<PluginError>> {
        let enabled = self.list_enabled();
        let mut errors = Vec::new();

        for entry in enabled {
            let plugin = &entry.plugin;
            let plugin_id_str = plugin.id.name.clone();
            let mut component_count = 0;
            let mut loaded_count = 0;

            // --- Tools ---
            for tool_path in &plugin.resolved_tools {
                component_count += 1;
                match crate::plugin::tool_loader::load_tool_spec(tool_path) {
                    Ok(mut spec) => {
                        spec.name = format!("plugin__{plugin_id_str}__{}", spec.name);
                        let cmd_tool =
                            crate::plugin::tool_loader::CommandTool::from_spec(spec, &plugin.path);
                        tools.register(cmd_tool);
                        loaded_count += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            plugin = %plugin.id,
                            tool = %tool_path.display(),
                            error = %e,
                            "failed to load plugin tool"
                        );
                    }
                }
            }

            // --- Unsupported component warnings ---
            if plugin.manifest.hooks.is_some() {
                tracing::warn!(
                    plugin = %plugin.id,
                    "plugin declares hooks but hook support is not yet implemented"
                );
            }
            if plugin.manifest.mcp_servers.is_some() {
                tracing::warn!(
                    plugin = %plugin.id,
                    "plugin declares MCP servers but MCP server support is not yet implemented"
                );
            }
            if plugin.manifest.lsp_servers.is_some() {
                tracing::warn!(
                    plugin = %plugin.id,
                    "plugin declares LSP servers but LSP server support is not yet implemented"
                );
            }
            if plugin.manifest.output_styles.is_some() {
                tracing::warn!(
                    plugin = %plugin.id,
                    "plugin declares output styles but output style support is not yet implemented"
                );
            }

            // --- Skills ---
            // Resolve skill paths: each entry can be a .md file or a directory.
            for skill_path in &plugin.resolved_skills {
                component_count += 1;
                let source = crate::skills::SkillSource::Plugin { plugin_id: plugin.id.clone() };
                if skill_path.is_dir() {
                    match skills.inject_skills_from_dir(skill_path, source) {
                        Ok(()) => loaded_count += 1,
                        Err(e) => {
                            tracing::warn!(
                                plugin = %plugin.id,
                                path = %skill_path.display(),
                                error = %e,
                                "failed to load plugin skills from directory"
                            );
                        }
                    }
                } else if skill_path.is_file() && skill_path.extension().is_some_and(|e| e == "md")
                {
                    // For single .md files, load from the containing directory
                    // (the SkillLoader scans all .md files in a directory)
                    if let Some(parent) = skill_path.parent() {
                        match skills.inject_skills_from_dir(parent, source) {
                            Ok(()) => loaded_count += 1,
                            Err(e) => {
                                tracing::warn!(
                                    plugin = %plugin.id,
                                    path = %parent.display(),
                                    error = %e,
                                    "failed to load plugin skill file"
                                );
                            }
                        }
                    }
                }
            }

            // --- Prompt sections ---
            for section_path in &plugin.resolved_prompt_sections {
                component_count += 1;
                if section_path.is_file() {
                    match std::fs::read_to_string(section_path) {
                        Ok(template) => {
                            let template =
                                template.replace("${PLUGIN_ROOT}", &plugin.path.to_string_lossy());
                            let stem = section_path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown");
                            let section = crate::plugin::PluginPromptSection {
                                name: format!("plugin_{plugin_id_str}_{stem}"),
                                template,
                            };
                            prompt.add(section);
                            loaded_count += 1;
                        }
                        Err(e) => {
                            tracing::warn!(
                                plugin = %plugin.id,
                                section = %section_path.display(),
                                error = %e,
                                "failed to read plugin prompt section"
                            );
                        }
                    }
                }
            }

            if component_count > 0 && loaded_count < component_count {
                errors.push(PluginError::Degraded {
                    id: plugin.id.clone(),
                    loaded: loaded_count,
                    total: component_count,
                });
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    /// Apply enabled plugin agent definitions into a subagent registry.
    ///
    /// Plugin agents are registered as `<plugin_name>:<agent_name>` so they do
    /// not collide with built-in, project, or user agent names.
    pub fn apply_subagents(
        &self,
        subagents: &mut SubagentRegistry,
    ) -> Result<(), Vec<PluginError>> {
        let mut errors = Vec::new();

        for entry in self.list_enabled() {
            let plugin = &entry.plugin;
            let mut component_count = 0;
            let mut loaded_count = 0;

            for agent_path in &plugin.resolved_agents {
                if agent_path.is_dir() {
                    let paths = match markdown_files(agent_path) {
                        Ok(paths) => paths,
                        Err(err) => {
                            errors.push(PluginError::ComponentLoadFailed(
                                plugin.id.clone(),
                                format!("failed to read agent dir {}: {err}", agent_path.display()),
                            ));
                            continue;
                        }
                    };
                    component_count += paths.len();
                    for path in paths {
                        match load_plugin_agent(&path, &plugin.id.name) {
                            Ok(agent) => {
                                subagents.register(agent);
                                loaded_count += 1;
                            }
                            Err(err) => {
                                errors.push(PluginError::ComponentLoadFailed(
                                    plugin.id.clone(),
                                    format!("failed to load agent {}: {err}", path.display()),
                                ));
                            }
                        }
                    }
                } else {
                    component_count += 1;
                    match load_plugin_agent(agent_path, &plugin.id.name) {
                        Ok(agent) => {
                            subagents.register(agent);
                            loaded_count += 1;
                        }
                        Err(err) => {
                            errors.push(PluginError::ComponentLoadFailed(
                                plugin.id.clone(),
                                format!("failed to load agent {}: {err}", agent_path.display()),
                            ));
                        }
                    }
                }
            }

            if component_count > 0 && loaded_count < component_count {
                errors.push(PluginError::Degraded {
                    id: plugin.id.clone(),
                    loaded: loaded_count,
                    total: component_count,
                });
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

fn load_plugin_agent(path: &Path, plugin_name: &str) -> Result<AgentDefinition, crate::AgentError> {
    let content = std::fs::read_to_string(path).map_err(|err| {
        crate::AgentError::Config(format!("failed to read agent file {}: {err}", path.display()))
    })?;
    let mut agent = AgentDefinition::from_markdown(
        &content,
        AgentSource::Plugin { plugin: plugin_name.to_string(), path: path.display().to_string() },
    )?;
    agent.name = format!("{plugin_name}:{}", agent.name);
    Ok(agent)
}

fn markdown_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, std::io::Error> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext == "md") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}
