use std::sync::Arc;

use anyhow::Result;
use telos_agent_host::runtime as shared_runtime;

use crate::config::FileConfig;
use crate::diagnostics::{self, DiagnosticsRuntime};

pub(crate) struct PreparedRuntime {
    pub shared: shared_runtime::PreparedRuntime,
    pub diagnostics: Option<DiagnosticsRuntime>,
}

pub(crate) fn prepare_runtime(
    options: &crate::cli::SharedOptions,
    file_config: &FileConfig,
    approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>>,
) -> Result<PreparedRuntime> {
    let runtime =
        shared_runtime::prepare_runtime(&options.to_runtime(), file_config, approval_handler)?;
    let mut agent_config = runtime.agent_config.clone();
    let diagnostics = diagnostics::configure_tool_diagnostics(
        &mut agent_config,
        file_config,
        runtime.project_root.as_deref(),
    )?;

    let mut runtime = runtime;
    runtime.agent_config = agent_config;
    Ok(PreparedRuntime { shared: runtime, diagnostics })
}

pub(crate) fn register_cli_subagent_tool(
    tools: &mut telos_agent::ToolRegistry,
    agent_config: &telos_agent::AgentConfig,
    provider: Arc<dyn telos_agent::ModelProvider + Send + Sync>,
) -> Result<()> {
    shared_runtime::register_subagent_tool(tools, agent_config, provider)
}

pub(crate) fn rebuild_prompt_assembly(runtime: &mut PreparedRuntime) {
    shared_runtime::rebuild_prompt_assembly(&mut runtime.shared);
}

pub(crate) async fn process_diagnostics(
    runtime: &Option<DiagnosticsRuntime>,
    file_config: &FileConfig,
) {
    if let Some(runtime) = runtime
        && let Err(err) = diagnostics::process_diagnostics(runtime, file_config).await
    {
        tracing::warn!("failed to process diagnostics: {err}");
    }
}
