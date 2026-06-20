#[derive(Clone)]
pub(super) struct ToolInfo {
    pub(super) name: String,
    pub(super) aliases: Vec<String>,
    pub(super) description: String,
}

pub(super) fn collect_tool_infos(tools: &telos_agent::ToolRegistry) -> Vec<ToolInfo> {
    let mut infos = tools
        .definitions()
        .into_iter()
        .map(|definition| {
            let aliases = tools
                .get(&definition.name)
                .map(|tool| tool.aliases().iter().map(|alias| (*alias).to_string()).collect())
                .unwrap_or_else(|_| Vec::new());
            ToolInfo { name: definition.name, aliases, description: definition.description }
        })
        .collect::<Vec<_>>();
    infos.sort_by(|a, b| a.name.cmp(&b.name));
    infos
}
