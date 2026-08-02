//! Apply enabled plugin components to agent registries.

use std::collections::HashMap;

use crate::agent::policies::{PolicyEntry, PolicyPoint};
use crate::integrations::mcp::McpServerConfig;
use crate::integrations::plugin::PluginError;
use crate::integrations::plugin::manifest::{
    LspServerEntry, LspServersConfig, McpServerEntry, McpServersConfig,
};
use crate::integrations::plugin::policy_loader::CommandPolicy;
use crate::integrations::plugin::registry::lifecycle::PluginRegistry;
use crate::orchestration::subagent::{AgentDefinition, AgentSource, SubagentRegistry};
use std::path::Path;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutputStyleDocument {
    name: Option<String>,
    instructions: String,
}

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
        _command_env: &HashMap<String, String>,
        skills: &mut crate::knowledge::skills::SkillRegistry,
        mcp: &mut crate::integrations::mcp::McpManager,
        prompt: &mut crate::agent::prompt::PromptAssembly,
    ) -> Result<(), Vec<PluginError>> {
        let enabled = self.list_enabled();
        let mut errors = Vec::new();

        for entry in enabled {
            let plugin = &entry.plugin;
            let error_start = errors.len();
            let plugin_id_str = plugin_component_namespace(&plugin.id);
            let mut component_count = 0;
            let mut loaded_count = 0;
            let resolved_config = match self
                .config_store
                .read()
                .expect("plugin config lock poisoned")
                .resolve(&plugin.id, &plugin.manifest)
            {
                Ok(config) => config,
                Err(error) => {
                    self.mark_error(&plugin.id, error.clone());
                    errors.push(error);
                    continue;
                }
            };
            let mut plugin_env = crate::config::platform_base_env();
            plugin_env.extend(resolved_config.command_env());
            plugin_env.insert("PLUGIN_ROOT".into(), plugin.path.to_string_lossy().into_owned());
            if plugin.manifest.settings.is_some() || plugin.manifest.user_config.is_some() {
                component_count += 1;
                loaded_count += 1;
            }

            // --- Tools ---
            for tool_path in &plugin.resolved_tools {
                component_count += 1;
                match crate::integrations::plugin::tool_loader::load_tool_spec(tool_path) {
                    Ok(mut spec) => {
                        spec.name = format!(
                            "plugin__{plugin_id_str}__{}",
                            normalize_component_name(&spec.name)
                        );
                        let cmd_tool =
                            crate::integrations::plugin::tool_loader::CommandTool::from_spec_with_env(
                                spec,
                                &plugin.path,
                                plugin_env.clone(),
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
                        errors.push(PluginError::ComponentLoadFailed(
                            plugin.id.clone(),
                            format!("tool {}: {e}", tool_path.display()),
                        ));
                    }
                }
            }

            // --- Policies ---
            if let Some(ref config) = plugin.manifest.policies {
                let policy_count = register_plugin_policies(
                    policies,
                    config,
                    &plugin_id_str,
                    &plugin.path,
                    &plugin_env,
                );
                if policy_count > 0 {
                    component_count += 1;
                    loaded_count += 1;
                }
            }

            // --- MCP Servers ---
            if let Some(ref mcp_servers) = plugin.manifest.mcp_servers {
                component_count += 1;
                if let Err(e) = register_plugin_mcp_servers(
                    mcp,
                    mcp_servers,
                    &plugin.path,
                    &plugin_id_str,
                    &plugin_env,
                ) {
                    tracing::warn!(
                        plugin = %plugin.id,
                        error = %e,
                        "failed to register plugin MCP servers"
                    );
                    errors.push(PluginError::ComponentLoadFailed(
                        plugin.id.clone(),
                        format!("MCP servers: {e}"),
                    ));
                } else {
                    loaded_count += 1;
                }
            }

            // --- LSP Servers ---
            if let Some(ref lsp_servers) = plugin.manifest.lsp_servers {
                match load_plugin_lsp_servers(lsp_servers, &plugin.path) {
                    Ok(servers) => {
                        component_count += servers.len();
                        for (name, entry) in servers {
                            let tool_name = format!(
                                "plugin__{plugin_id_str}__lsp__{}",
                                normalize_component_name(&name)
                            );
                            match crate::integrations::plugin::LspTool::new(
                                tool_name,
                                &name,
                                entry,
                                &plugin.path,
                                plugin_env.clone(),
                            ) {
                                Ok(tool) => {
                                    tools.register(tool);
                                    loaded_count += 1;
                                }
                                Err(error) => errors.push(PluginError::ComponentLoadFailed(
                                    plugin.id.clone(),
                                    error.to_string(),
                                )),
                            }
                        }
                    }
                    Err(error) => {
                        component_count += 1;
                        errors.push(error);
                    }
                }
            }

            // --- Output Styles ---
            for style_path in &plugin.resolved_output_styles {
                component_count += 1;
                match load_output_style(style_path, &plugin_id_str, &plugin.path, &resolved_config)
                {
                    Ok(section) => {
                        prompt.add(section);
                        loaded_count += 1;
                    }
                    Err(error) => {
                        tracing::warn!(plugin = %plugin.id, style = %style_path.display(), error = %error, "failed to load plugin output style");
                        errors.push(PluginError::ComponentLoadFailed(
                            plugin.id.clone(),
                            error.to_string(),
                        ));
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
                            errors.push(PluginError::ComponentLoadFailed(
                                plugin.id.clone(),
                                format!("skills {}: {e}", skill_path.display()),
                            ));
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
                        errors.push(PluginError::ComponentLoadFailed(
                            plugin.id.clone(),
                            format!("failed to parse skill {}", skill_path.display()),
                        ));
                    }
                } else {
                    errors.push(PluginError::ComponentLoadFailed(
                        plugin.id.clone(),
                        format!(
                            "skill path {} is not a Markdown file or directory",
                            skill_path.display()
                        ),
                    ));
                }
            }

            // --- Prompt sections ---
            for section_path in &plugin.resolved_prompt_sections {
                component_count += 1;
                if section_path.is_file() {
                    match std::fs::read_to_string(section_path) {
                        Ok(template) => {
                            let template = render_plugin_template(
                                &template.replace("${PLUGIN_ROOT}", &plugin.path.to_string_lossy()),
                                &resolved_config,
                            );
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
                            errors.push(PluginError::ComponentLoadFailed(
                                plugin.id.clone(),
                                format!("prompt section {}: {e}", section_path.display()),
                            ));
                        }
                    }
                } else {
                    errors.push(PluginError::ComponentLoadFailed(
                        plugin.id.clone(),
                        format!("prompt section {} is not a file", section_path.display()),
                    ));
                }
            }

            if component_count > 0 && loaded_count < component_count {
                errors.push(PluginError::Degraded {
                    id: plugin.id.clone(),
                    loaded: loaded_count,
                    total: component_count,
                });
            }
            if errors.len() == error_start {
                self.mark_loaded(&plugin.id);
            } else {
                self.mark_degraded(&plugin.id, errors[error_start..].to_vec());
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
            let plugin_id_str = plugin_component_namespace(&plugin.id);
            let Ok(resolved_config) = self
                .config_store
                .read()
                .expect("plugin config lock poisoned")
                .resolve(&plugin.id, &plugin.manifest)
            else {
                continue;
            };
            for section_path in &plugin.resolved_prompt_sections {
                if section_path.is_file()
                    && let Ok(template) = std::fs::read_to_string(section_path)
                {
                    let template = render_plugin_template(
                        &template.replace("${PLUGIN_ROOT}", &plugin.path.to_string_lossy()),
                        &resolved_config,
                    );
                    let stem =
                        section_path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
                    let section = crate::integrations::plugin::PluginPromptSection {
                        name: format!("plugin_{plugin_id_str}_{stem}"),
                        template,
                    };
                    prompt.add(section);
                }
            }
            for style_path in &plugin.resolved_output_styles {
                if let Ok(section) =
                    load_output_style(style_path, &plugin_id_str, &plugin.path, &resolved_config)
                {
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
            let error_start = errors.len();
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
                        match load_plugin_agent(&path, &plugin.id) {
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
                    match load_plugin_agent(agent_path, &plugin.id) {
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
            if errors.len() > error_start {
                let mut plugin_errors = entry.load_errors.clone();
                plugin_errors.extend_from_slice(&errors[error_start..]);
                self.mark_degraded(&plugin.id, plugin_errors);
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
        let name = command.name.as_ref().map_or_else(
            || format!("plugin::{plugin}::policy::{count}"),
            |name| format!("plugin::{plugin}::{name}"),
        );
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
    for item in &config.session_end {
        add(PolicyPoint::SessionEnd, item);
    }
    for item in &config.turn_start {
        add(PolicyPoint::TurnStart, item);
    }
    for item in &config.model_before_request {
        add(PolicyPoint::ModelBeforeRequest, item);
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
    plugin_env: &HashMap<String, String>,
) -> Result<(), PluginError> {
    let servers = match mcp_servers {
        McpServersConfig::Inline(map) => map.clone(),
        McpServersConfig::File(rel_path) => {
            let abs_path = safe_plugin_file(plugin_path, rel_path)?;
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
    let mut validation_errors = Vec::new();
    for (name, entry) in &servers {
        if name.trim().is_empty() {
            validation_errors.push("MCP server name must not be empty".into());
        }
        if entry.command.trim().is_empty() {
            validation_errors.push(format!("MCP server `{name}` command must not be empty"));
        }
        if entry.timeout_ms == 0 {
            validation_errors
                .push(format!("MCP server `{name}` timeoutMs must be greater than zero"));
        }
        if entry.env.keys().any(|key| key.is_empty() || key.contains('=') || key.contains('\0')) {
            validation_errors.push(format!("MCP server `{name}` has an invalid environment key"));
        }
    }
    if !validation_errors.is_empty() {
        return Err(PluginError::ManifestValidation { errors: validation_errors });
    }
    let namespace_id = |name: &str| -> String {
        format!("plugin__{plugin_id_str}__{}", normalize_component_name(name))
    };
    let server_configs: HashMap<String, McpServerConfig> = servers
        .into_iter()
        .map(|(name, entry): (String, McpServerEntry)| {
            (namespace_id(&name), mcp_server_entry_to_config(entry, plugin_env, plugin_path))
        })
        .collect();
    tokio::task::block_in_place(move || {
        tokio::runtime::Handle::current().block_on(async {
            mcp.register_servers(server_configs).await;
        })
    });
    Ok(())
}

fn load_plugin_lsp_servers(
    config: &LspServersConfig,
    plugin_root: &Path,
) -> Result<HashMap<String, LspServerEntry>, PluginError> {
    match config {
        LspServersConfig::Inline(servers) => Ok(servers.clone()),
        LspServersConfig::File(relative) => {
            let path = safe_plugin_file(plugin_root, relative)?;
            let content = std::fs::read_to_string(&path).map_err(|error| {
                PluginError::Io(format!("failed to read LSP config {}: {error}", path.display()))
            })?;
            let value: serde_json::Value = serde_json::from_str(&content).map_err(|error| {
                PluginError::Json(format!("invalid LSP config {}: {error}", path.display()))
            })?;
            serde_json::from_value(value.get("lspServers").cloned().unwrap_or(value)).map_err(
                |error| {
                    PluginError::Json(format!(
                        "failed to decode LSP config {}: {error}",
                        path.display()
                    ))
                },
            )
        }
    }
}

fn safe_plugin_file(plugin_root: &Path, relative: &str) -> Result<std::path::PathBuf, PluginError> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(PluginError::ManifestValidation {
            errors: vec![format!(
                "plugin config path `{}` escapes plugin root",
                relative.display()
            )],
        });
    }
    let path = plugin_root.join(relative);
    let root = std::fs::canonicalize(plugin_root)?;
    let path = std::fs::canonicalize(&path)?;
    if !path.starts_with(&root) {
        return Err(PluginError::ManifestValidation {
            errors: vec![format!(
                "plugin config path `{}` resolves outside plugin root",
                relative.display()
            )],
        });
    }
    Ok(path)
}

pub(super) fn normalize_component_name(name: &str) -> String {
    let mut normalized = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' {
            normalized.push(char::from(byte));
        } else {
            normalized.push_str(&format!("_x{byte:02x}"));
        }
    }
    normalized
}

fn plugin_component_namespace(id: &crate::integrations::plugin::PluginId) -> String {
    format!("{}__{}", normalize_component_name(&id.name), normalize_component_name(&id.marketplace))
}

fn mcp_server_entry_to_config(
    entry: McpServerEntry,
    plugin_env: &HashMap<String, String>,
    plugin_root: &Path,
) -> McpServerConfig {
    let root = plugin_root.to_string_lossy();
    let mut env = plugin_env.clone();
    env.extend(
        entry.env.into_iter().map(|(key, value)| (key, value.replace("${PLUGIN_ROOT}", &root))),
    );
    let command = entry.command.replace("${PLUGIN_ROOT}", &root);
    let command_path = Path::new(&command);
    let command = if command_path.is_relative()
        && (command.starts_with('.') || command.contains('/') || command.contains('\\'))
    {
        plugin_root.join(command_path).to_string_lossy().into_owned()
    } else {
        command
    };
    McpServerConfig {
        command,
        args: entry
            .args
            .into_iter()
            .map(|argument| argument.replace("${PLUGIN_ROOT}", &root))
            .collect(),
        env,
        inherit_env: false,
        cwd: Some(plugin_root.to_path_buf()),
        auto_connect: entry.auto_connect,
        timeout_ms: entry.timeout_ms,
    }
}

fn load_output_style(
    path: &Path,
    plugin_name: &str,
    plugin_root: &Path,
    config: &crate::integrations::plugin::ResolvedPluginConfig,
) -> Result<crate::integrations::plugin::PluginPromptSection, PluginError> {
    let content = std::fs::read_to_string(path).map_err(|error| {
        PluginError::Io(format!("failed to read output style {}: {error}", path.display()))
    })?;
    let fallback_name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("style");
    let (name, instructions) = if path.extension().is_some_and(|extension| extension == "json") {
        let document: OutputStyleDocument = serde_json::from_str(&content).map_err(|error| {
            PluginError::Json(format!("invalid output style {}: {error}", path.display()))
        })?;
        (document.name.unwrap_or_else(|| fallback_name.into()), document.instructions)
    } else {
        (fallback_name.into(), content)
    };
    let instructions = render_plugin_template(
        &instructions.replace("${PLUGIN_ROOT}", &plugin_root.to_string_lossy()),
        config,
    );
    if instructions.trim().is_empty() {
        return Err(PluginError::Other(format!(
            "output style {} has no instructions",
            path.display()
        )));
    }
    Ok(crate::integrations::plugin::PluginPromptSection {
        name: format!("plugin_{plugin_name}_output_style_{name}"),
        template: instructions,
    })
}

fn render_plugin_template(
    template: &str,
    config: &crate::integrations::plugin::ResolvedPluginConfig,
) -> String {
    config.render_template(template)
}

fn load_plugin_agent(
    path: &Path,
    plugin_id: &crate::integrations::plugin::PluginId,
) -> Result<AgentDefinition, crate::AgentError> {
    let content = std::fs::read_to_string(path).map_err(|err| {
        crate::AgentError::Config(format!("failed to read agent file {}: {err}", path.display()))
    })?;
    let mut agent = AgentDefinition::from_markdown(
        &content,
        AgentSource::Plugin { plugin: plugin_id.to_string(), path: path.display().to_string() },
    )?;
    agent.name = format!("{plugin_id}:{}", agent.name);
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
