//! Apply enabled plugin components to agent registries.

use std::collections::HashMap;

use crate::agent::policies::{PolicyEntry, PolicyPoint};
use crate::integrations::mcp::McpServerConfig;
use crate::integrations::plugin::PluginError;
use crate::integrations::plugin::manifest::{McpServerEntry, McpServersConfig};
use crate::integrations::plugin::policy_loader::CommandPolicy;
use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::orchestration::subagent::{AgentDefinition, AgentSource, SubagentRegistry};
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
        tools: &mut crate::tools::api::ToolRegistry,
        policies: &mut crate::agent::policies::PolicyRegistry,
        command_env: &HashMap<String, String>,
        skills: &mut crate::knowledge::skills::SkillRegistry,
        mcp: &mut crate::integrations::mcp::McpManager,
        prompt: &mut crate::agent::prompt::PromptAssembly,
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
                match crate::integrations::plugin::tool_loader::load_tool_spec(tool_path) {
                    Ok(mut spec) => {
                        spec.name = format!("plugin__{plugin_id_str}__{}", spec.name);
                        let cmd_tool =
                            crate::integrations::plugin::tool_loader::CommandTool::from_spec(
                                spec,
                                &plugin.path,
                            );
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

            // --- Policies ---
            if let Some(ref config) = plugin.manifest.policies {
                component_count += 1;
                let policy_count = register_plugin_policies(
                    policies,
                    config,
                    &plugin_id_str,
                    &plugin.path,
                    command_env,
                );
                if policy_count > 0 {
                    loaded_count += 1;
                }
            }

            // --- MCP Servers ---
            if let Some(ref mcp_servers) = plugin.manifest.mcp_servers {
                component_count += 1;
                if let Err(e) =
                    register_plugin_mcp_servers(mcp, mcp_servers, &plugin.path, &plugin_id_str)
                {
                    tracing::warn!(
                        plugin = %plugin.id,
                        error = %e,
                        "failed to register plugin MCP servers"
                    );
                } else {
                    loaded_count += 1;
                }
            }

            // --- LSP Servers ---
            if plugin.manifest.lsp_servers.is_some() {
                component_count += 1;
                tracing::info!(
                    plugin = %plugin.id,
                    "plugin declares LSP servers — LSP integration not yet wired into the agent runtime"
                );
                loaded_count += 1;
            }

            // --- Output Styles ---
            if let Some(ref styles) = plugin.manifest.output_styles {
                component_count += styles.len();
                for style_path in styles {
                    let abs_path = plugin.path.join(style_path);
                    if abs_path.exists() {
                        tracing::info!(
                            plugin = %plugin.id,
                            style = %style_path,
                            "plugin output style available (not yet wired into the agent runtime)"
                        );
                        loaded_count += 1;
                    } else {
                        tracing::warn!(
                            plugin = %plugin.id,
                            style = %style_path,
                            "plugin output style file not found"
                        );
                    }
                }
            }

            // --- Skills ---
            // Resolve skill paths: each entry can be a .md file or a directory.
            for skill_path in &plugin.resolved_skills {
                component_count += 1;
                let source =
                    crate::knowledge::skills::SkillSource::Plugin { plugin_id: plugin.id.clone() };
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
                    if let Some(skill) =
                        crate::knowledge::skills::SkillLoader::load_skill_file(skill_path, source)
                    {
                        skills.register(skill);
                        loaded_count += 1;
                    } else {
                        tracing::warn!(
                            plugin = %plugin.id,
                            path = %skill_path.display(),
                            "failed to parse plugin skill file"
                        );
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
                            let section = crate::integrations::plugin::PluginPromptSection {
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

    /// Re-apply only prompt sections from enabled plugins into a prompt assembly.
    ///
    /// This is a lighter variant of [`apply`](Self::apply) — it does not
    /// re-register tools, policies, skills, or MCP servers. Useful when the
    /// prompt assembly is rebuilt (e.g. after tools change).
    pub fn apply_prompt_sections(&self, prompt: &mut crate::agent::prompt::PromptAssembly) {
        for entry in self.list_enabled() {
            let plugin = &entry.plugin;
            let plugin_id_str = plugin.id.name.clone();
            for section_path in &plugin.resolved_prompt_sections {
                if section_path.is_file()
                    && let Ok(template) = std::fs::read_to_string(section_path)
                {
                    let template =
                        template.replace("${PLUGIN_ROOT}", &plugin.path.to_string_lossy());
                    let stem =
                        section_path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
                    let section = crate::integrations::plugin::PluginPromptSection {
                        name: format!("plugin_{plugin_id_str}_{stem}"),
                        template,
                    };
                    prompt.add(section);
                }
            }
        }
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

fn register_plugin_policies(
    registry: &mut crate::agent::policies::PolicyRegistry,
    config: &crate::integrations::plugin::manifest::PoliciesConfig,
    plugin: &str,
    plugin_root: &Path,
    command_env: &HashMap<String, String>,
) -> usize {
    let mut count = 0;
    let mut add = |point, command: &crate::integrations::plugin::manifest::CommandPolicyDef| {
        let name = format!("plugin_{plugin}_policy_{count}");
        registry.register(PolicyEntry {
            point,
            policy: std::sync::Arc::new(CommandPolicy::new(
                name,
                command.command.clone(),
                command.args.clone(),
                command.timeout,
                plugin_root.to_path_buf(),
                command_env.clone(),
            )),
        });
        count += 1;
    };
    for item in &config.session_start {
        add(PolicyPoint::SessionStart { mode: item.mode }, &item.command);
    }
    for item in &config.model_response {
        add(PolicyPoint::ModelResponse, item);
    }
    for item in &config.tool_before_invoke {
        add(PolicyPoint::ToolBeforeInvoke { matcher: item.matcher.clone() }, &item.command);
    }
    for item in &config.tool_after_invoke {
        add(PolicyPoint::ToolAfterInvoke { matcher: item.matcher.clone() }, &item.command);
    }
    for item in &config.turn_before_finish {
        add(PolicyPoint::TurnBeforeFinish, item);
    }
    count
}

/// Register MCP servers declared by a plugin into the MCP manager.
fn register_plugin_mcp_servers(
    mcp: &crate::integrations::mcp::McpManager,
    mcp_servers: &McpServersConfig,
    plugin_path: &Path,
    plugin_id_str: &str,
) -> Result<(), PluginError> {
    let servers = match mcp_servers {
        McpServersConfig::Inline(map) => map.clone(),
        McpServersConfig::File(rel_path) => {
            let abs_path = plugin_path.join(rel_path);
            let content = std::fs::read_to_string(&abs_path).map_err(|e| {
                PluginError::Io(format!(
                    "failed to read plugin MCP config {}: {e}",
                    abs_path.display()
                ))
            })?;
            let value: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
                PluginError::Json(format!(
                    "failed to parse plugin MCP config {}: {e}",
                    abs_path.display()
                ))
            })?;
            let config_val = value.get("mcpServers").unwrap_or(&value);
            serde_json::from_value(config_val.clone()).map_err(|e| {
                PluginError::Json(format!("failed to decode plugin MCP servers: {e}"))
            })?
        }
    };
    let namespace_id = |name: &str| -> String { format!("plugin__{plugin_id_str}__{name}") };
    let server_configs: HashMap<String, McpServerConfig> = servers
        .into_iter()
        .map(|(name, entry): (String, McpServerEntry)| {
            (namespace_id(&name), mcp_server_entry_to_config(entry))
        })
        .collect();
    tokio::task::block_in_place(move || {
        tokio::runtime::Handle::current().block_on(async {
            mcp.register_servers(server_configs).await;
        })
    });
    Ok(())
}

fn mcp_server_entry_to_config(entry: McpServerEntry) -> McpServerConfig {
    McpServerConfig {
        command: entry.command,
        args: entry.args,
        env: entry.env,
        cwd: None,
        auto_connect: entry.auto_connect,
        timeout_ms: entry.timeout_ms,
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
