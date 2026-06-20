use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use telos_agent::{
    AgentConfig, AgentSession, ApprovalDecision, AutoDenyHandler, CompletionResponse,
    FixedDecisionHandler, JsonlStorage, Message, MockProvider, ModelProvider, StopReason,
    ToolRegistry,
};
use telos_cli::cli::{ProviderArg, SharedOptions};
use telos_cli::config::{self, FileConfig, ResolvedProvider};

use crate::desktop_event::{DesktopEvent, map_turn_event};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    Deepseek,
    Mock,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSettingsOverrides {
    pub provider: Option<ProviderKind>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<PathBuf>,
    pub max_iterations: Option<usize>,
    pub auto_approve: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDesktopSettings {
    pub provider: ProviderKind,
    pub model: String,
    pub cwd: PathBuf,
    pub project_root: Option<PathBuf>,
    pub project_root_or_cwd: PathBuf,
    pub memory_root: PathBuf,
    pub memory_count: usize,
    pub api_key_configured: bool,
    pub auto_approve: bool,
    pub max_iterations: usize,
    pub config_path: Option<PathBuf>,
    pub instructions_file: Option<String>,
}

pub struct AgentHost {
    session: AgentSession,
    provider: Arc<dyn ModelProvider>,
    tools: ToolRegistry,
    memory_store: Arc<std::sync::Mutex<telos_agent::MemoryStore>>,
    tool_details: HashMap<String, String>,
}

impl AgentHost {
    pub fn new(overrides: DesktopSettingsOverrides) -> Result<Self, String> {
        let resolved = resolve_desktop_settings(&overrides)?;
        let shared = shared_options(&overrides, &resolved);
        let merged = load_merged_config(&resolved.cwd)?;
        let approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>> =
            Some(if resolved.auto_approve {
                Arc::new(FixedDecisionHandler { decision: ApprovalDecision::Allow })
            } else {
                Arc::new(AutoDenyHandler)
            });

        let mut runtime = prepare_desktop_runtime(&shared, &merged, approval_handler)?;
        let sessions_dir = resolved.project_root_or_cwd.join(".telos").join("desktop-sessions");
        runtime.agent_config.storage =
            Some(Arc::new(JsonlStorage::new(sessions_dir).map_err(|e| e.to_string())?));

        let provider = match config::build_provider(&shared, &merged).map_err(|e| e.to_string())? {
            ResolvedProvider::DeepSeek(provider) => Arc::new(provider) as Arc<dyn ModelProvider>,
            ResolvedProvider::Routed(provider) => Arc::new(provider) as Arc<dyn ModelProvider>,
            ResolvedProvider::Mock(_) => Arc::new(MockProvider::new(vec![CompletionResponse {
                message: Message::assistant("桌面端当前使用 Mock Provider，没有真实模型调用。"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            }])) as Arc<dyn ModelProvider>,
        };

        let session = AgentSession::new(runtime.agent_config).map_err(|e| e.to_string())?;

        Ok(Self {
            session,
            provider,
            tools: runtime.tools,
            memory_store: runtime.memory_store,
            tool_details: HashMap::new(),
        })
    }

    pub async fn run_prompt<F>(&mut self, prompt: String, mut on_event: F) -> Result<String, String>
    where
        F: FnMut(DesktopEvent),
    {
        telos_cli::memory_runtime::record_user_preference(&self.memory_store, &prompt).await;

        let erased = telos_agent::ErasedProvider(self.provider.as_ref());
        let mut final_text = String::new();
        let memory_store = self.memory_store.clone();
        let mut tool_details = std::mem::take(&mut self.tool_details);
        {
            let mut stream = pin!(self.session.run_turn_stream(&erased, &self.tools, prompt));
            while let Some(event) = stream.next().await {
                let event = event.map_err(|err| err.to_string())?;
                record_memory_from_event(&memory_store, &mut tool_details, &event).await;
                if let telos_agent::TurnEvent::TurnFinished { final_text: text, .. } = &event {
                    final_text = text.clone();
                }
                let desktop_event = map_turn_event(event);
                if desktop_event.kind != "ignored" {
                    on_event(desktop_event);
                }
            }
        }
        self.tool_details = tool_details;
        self.session.save().await.map_err(|err| err.to_string())?;
        Ok(final_text)
    }
}

async fn record_memory_from_event(
    memory_store: &Arc<std::sync::Mutex<telos_agent::MemoryStore>>,
    tool_details: &mut HashMap<String, String>,
    event: &telos_agent::TurnEvent,
) {
    match event {
        telos_agent::TurnEvent::ToolCall { tool_call_id, detail, .. } => {
            tool_details.insert(tool_call_id.clone(), detail.clone());
        }
        telos_agent::TurnEvent::ToolCompleted { tool_call_id, name, is_error: false } => {
            telos_cli::memory_runtime::record_successful_tool(
                memory_store,
                name,
                tool_call_id,
                tool_details.get(tool_call_id).map(String::as_str),
            )
            .await;
        }
        telos_agent::TurnEvent::ToolResult(message) => {
            for result in message.tool_results_iter() {
                if result.is_error {
                    telos_cli::memory_runtime::record_tool_error(
                        memory_store,
                        result,
                        tool_details.get(&result.tool_call_id).map(String::as_str),
                    )
                    .await;
                }
            }
        }
        _ => {}
    }
}

struct PreparedDesktopRuntime {
    agent_config: AgentConfig,
    tools: ToolRegistry,
    memory_store: Arc<std::sync::Mutex<telos_agent::MemoryStore>>,
}

pub fn resolve_desktop_settings(
    overrides: &DesktopSettingsOverrides,
) -> Result<ResolvedDesktopSettings, String> {
    let cwd = clean_path(overrides.cwd.clone().unwrap_or_else(telos_cli::config::default_cwd));
    let merged = load_merged_config(&cwd)?;
    let shared = shared_options(overrides, &settings_from_config(&merged, cwd.clone())?);
    let project_root = telos_cli::find_project_root(&cwd).ok().map(clean_path);
    let project_root_or_cwd = project_root.clone().unwrap_or_else(|| cwd.clone());
    let memory_root =
        clean_path(telos_cli::memory_runtime::memory_root(project_root.as_deref()).map_err(|e| e.to_string())?);
    let memory_count = telos_agent::MemoryStore::new(memory_root.clone()).list().len();
    let context = project_root
        .as_deref()
        .map(telos_cli::context::load_project_context)
        .unwrap_or_else(telos_cli::context::ProjectContext::empty);

    let provider = overrides
        .provider
        .or_else(|| provider_from_file_config(&merged))
        .unwrap_or(ProviderKind::Deepseek);
    let model = shared
        .model
        .clone()
        .or_else(|| merged.agent.as_ref()?.model.clone())
        .unwrap_or_else(|| "auto".to_string());
    let auto_approve = overrides.auto_approve.or(merged.auto_mode).unwrap_or(false);
    let max_iterations = shared
        .max_iterations
        .or_else(|| merged.agent.as_ref()?.max_iterations)
        .unwrap_or(30);

    Ok(ResolvedDesktopSettings {
        provider,
        model,
        cwd,
        project_root,
        project_root_or_cwd,
        memory_root,
        memory_count,
        api_key_configured: has_deepseek_api_key(overrides, &merged),
        auto_approve,
        max_iterations,
        config_path: user_config_path(),
        instructions_file: context.instructions_file,
    })
}

pub fn save_deepseek_api_key(api_key: &str) -> Result<ResolvedDesktopSettings, String> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err("DeepSeek API Key 不能为空".into());
    }

    let path = user_config_path().ok_or_else(|| "无法确定用户配置目录".to_string())?;
    let mut config = if path.exists() {
        config::load_config_file(&path).map_err(|e| e.to_string())?.unwrap_or_default()
    } else {
        FileConfig::default()
    };
    let mut agent = config.agent.unwrap_or_default();
    agent.provider = Some("deepseek".into());
    agent.model = agent.model.or_else(|| Some("auto".into()));
    config.agent = Some(agent);
    let mut env = config.env.unwrap_or_default();
    env.insert("DEEPSEEK_API_KEY".into(), trimmed.to_string());
    config.env = Some(env);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let serialized = toml::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, serialized).map_err(|e| e.to_string())?;

    resolve_desktop_settings(&DesktopSettingsOverrides::default())
}

fn prepare_desktop_runtime(
    options: &SharedOptions,
    file_config: &FileConfig,
    approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>>,
) -> Result<PreparedDesktopRuntime, String> {
    let cwd = options.cwd.clone().unwrap_or_else(telos_cli::config::default_cwd);
    let project_root = telos_cli::find_project_root(&cwd).ok();
    let project_root_or_cwd = project_root.clone().unwrap_or_else(|| cwd.clone());
    let context = project_root
        .as_deref()
        .map(telos_cli::context::load_project_context)
        .unwrap_or_else(telos_cli::context::ProjectContext::empty);
    let memory_store =
        telos_cli::memory_runtime::open_memory_store(project_root.as_deref()).map_err(|e| e.to_string())?;

    let mut agent_config =
        config::build_agent_config(options, file_config, approval_handler).map_err(|e| e.to_string())?;
    let mut tools = ToolRegistry::new();
    telos_agent::register_core_tools(&mut tools);
    let task_manager = Arc::new(telos_agent::TaskManager::new(
        project_root_or_cwd.join(".telos").join("tasks"),
    ));
    telos_agent::register_task_tools(&mut tools, task_manager);

    let mut assembly = telos_cli::context::build_prompt_assembly(&context);
    telos_cli::memory_runtime::register_memory_runtime(
        &mut tools,
        &mut assembly,
        memory_store.clone(),
    );
    agent_config.prompt_assembly = Some(Arc::new(assembly));

    Ok(PreparedDesktopRuntime { agent_config, tools, memory_store })
}

fn load_merged_config(cwd: &Path) -> Result<FileConfig, String> {
    let user = config::load_user_config(None).map_err(|e| e.to_string())?;
    let project = telos_cli::find_project_root(cwd)
        .ok()
        .map(|root| config::load_project_config(&root).map_err(|e| e.to_string()))
        .transpose()?
        .flatten();
    Ok(config::merge_configs(user, project))
}

fn settings_from_config(
    config: &FileConfig,
    cwd: PathBuf,
) -> Result<ResolvedDesktopSettings, String> {
    let cwd = clean_path(cwd);
    let project_root = telos_cli::find_project_root(&cwd).ok().map(clean_path);
    let project_root_or_cwd = project_root.clone().unwrap_or_else(|| cwd.clone());
    let memory_root =
        clean_path(telos_cli::memory_runtime::memory_root(project_root.as_deref()).map_err(|e| e.to_string())?);
    Ok(ResolvedDesktopSettings {
        provider: provider_from_file_config(config).unwrap_or(ProviderKind::Deepseek),
        model: config
            .agent
            .as_ref()
            .and_then(|agent| agent.model.clone())
            .unwrap_or_else(|| "auto".into()),
        cwd,
        project_root,
        project_root_or_cwd,
        memory_root,
        memory_count: 0,
        api_key_configured: has_deepseek_api_key(&DesktopSettingsOverrides::default(), config),
        auto_approve: config.auto_mode.unwrap_or(false),
        max_iterations: config.agent.as_ref().and_then(|a| a.max_iterations).unwrap_or(30),
        config_path: user_config_path(),
        instructions_file: None,
    })
}

fn shared_options(
    overrides: &DesktopSettingsOverrides,
    resolved: &ResolvedDesktopSettings,
) -> SharedOptions {
    SharedOptions {
        provider: Some(match overrides.provider.unwrap_or(resolved.provider) {
            ProviderKind::Deepseek => ProviderArg::Deepseek,
            ProviderKind::Mock => ProviderArg::Mock,
        }),
        model: overrides.model.clone().or_else(|| Some(resolved.model.clone())),
        api_key: overrides
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(ToOwned::to_owned),
        cwd: Some(overrides.cwd.clone().unwrap_or_else(|| resolved.cwd.clone())),
        max_iterations: overrides.max_iterations.or(Some(resolved.max_iterations)),
        ..SharedOptions::default()
    }
}

fn provider_from_file_config(config: &FileConfig) -> Option<ProviderKind> {
    match config.agent.as_ref()?.provider.as_deref()?.trim().to_lowercase().as_str() {
        "deepseek" | "deep" => Some(ProviderKind::Deepseek),
        "mock" => Some(ProviderKind::Mock),
        _ => None,
    }
}

fn has_deepseek_api_key(overrides: &DesktopSettingsOverrides, config: &FileConfig) -> bool {
    if overrides.api_key.as_deref().map(str::trim).is_some_and(|key| !key.is_empty()) {
        return true;
    }
    if std::env::var("DEEPSEEK_API_KEY").is_ok_and(|key| !key.trim().is_empty()) {
        return true;
    }
    config
        .env
        .as_ref()
        .and_then(|env| env.get("DEEPSEEK_API_KEY"))
        .map(String::as_str)
        .map(str::trim)
        .is_some_and(|key| !key.is_empty())
}

fn user_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|base| clean_path(base.join("telos").join("config.toml")))
}

fn clean_path(path: PathBuf) -> PathBuf {
    dunce::simplified(&path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_host_runs_prompt_and_emits_finish_event() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let mut host = AgentHost::new(DesktopSettingsOverrides {
            provider: Some(ProviderKind::Mock),
            cwd: Some(temp.path().to_path_buf()),
            ..DesktopSettingsOverrides::default()
        })
        .expect("mock host should initialize");

        let mut events = Vec::new();
        let final_text = host
            .run_prompt("hello".to_string(), |event| events.push(event))
            .await
            .expect("mock prompt should run");

        assert_eq!(final_text, "桌面端当前使用 Mock Provider，没有真实模型调用。");
        assert!(events.iter().any(|event| event.kind == "turn_finished"));
    }

    #[test]
    fn project_config_controls_resolved_provider_and_model() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        std::fs::write(
            temp.path().join(".telos.toml"),
            r#"
[agent]
provider = "mock"
model = "deepseek-v4-flash"
max_iterations = 9
"#,
        )
        .unwrap();

        let resolved = resolve_desktop_settings(&DesktopSettingsOverrides {
            cwd: Some(temp.path().to_path_buf()),
            ..DesktopSettingsOverrides::default()
        })
        .unwrap();

        assert_eq!(resolved.provider, ProviderKind::Mock);
        assert_eq!(resolved.model, "deepseek-v4-flash");
        assert_eq!(resolved.max_iterations, 9);
        assert_eq!(resolved.project_root.as_deref(), Some(temp.path()));
    }

    #[test]
    fn project_memory_root_matches_cli_location() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        std::fs::write(temp.path().join(".telos.toml"), "").unwrap();

        let resolved = resolve_desktop_settings(&DesktopSettingsOverrides {
            cwd: Some(temp.path().to_path_buf()),
            ..DesktopSettingsOverrides::default()
        })
        .unwrap();

        assert_eq!(resolved.memory_root, temp.path().join(".telos").join("memory"));
    }
}
