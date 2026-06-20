use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use telos_agent::{
    AgentConfig, AgentSession, ApprovalDecision, AutoDenyHandler, CompletionResponse,
    DeepSeekConfig, DeepSeekProvider, ErasedProvider, FixedDecisionHandler, JsonlStorage, Message,
    MockProvider, ModelProvider, RoutedModelConfig, RoutedProvider, StopReason, ToolRegistry,
};

use crate::desktop_event::{DesktopEvent, map_turn_event};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    Mock,
    Deepseek,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSettings {
    pub provider: ProviderKind,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<PathBuf>,
    pub max_iterations: Option<usize>,
    pub auto_approve: bool,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Mock,
            api_key: None,
            model: Some("auto".into()),
            cwd: None,
            max_iterations: Some(30),
            auto_approve: false,
        }
    }
}

pub struct AgentHost {
    session: AgentSession,
    provider: Arc<dyn ModelProvider>,
    tools: ToolRegistry,
}

impl AgentHost {
    pub fn new(settings: ChatSettings) -> Result<Self, String> {
        let cwd = settings
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let mut config = AgentConfig {
            cwd: cwd.clone(),
            max_iterations: settings.max_iterations.unwrap_or(30),
            ..AgentConfig::default()
        };

        if let Ok(path) = std::env::var("PATH") {
            config.env.insert("PATH".into(), path);
        }
        if let Ok(home) = std::env::var("HOME") {
            config.env.insert("HOME".into(), home);
        }

        config.approval_handler = Some(if settings.auto_approve {
            Arc::new(FixedDecisionHandler { decision: ApprovalDecision::Allow })
        } else {
            Arc::new(AutoDenyHandler)
        });

        let sessions_dir = cwd.join(".telos").join("desktop-sessions");
        config.storage =
            Some(Arc::new(JsonlStorage::new(sessions_dir).map_err(|e| e.to_string())?));

        let mut tools = ToolRegistry::new();
        telos_agent::register_core_tools(&mut tools);
        let task_manager =
            Arc::new(telos_agent::TaskManager::new(cwd.join(".telos").join("tasks")));
        telos_agent::register_task_tools(&mut tools, task_manager);

        let provider = build_provider(&settings)?;
        let session = AgentSession::new(config).map_err(|e| e.to_string())?;

        Ok(Self { session, provider, tools })
    }

    pub async fn run_prompt<F>(&mut self, prompt: String, mut on_event: F) -> Result<String, String>
    where
        F: FnMut(DesktopEvent),
    {
        let erased = ErasedProvider(self.provider.as_ref());
        let mut final_text = String::new();
        {
            let mut stream = pin!(self.session.run_turn_stream(&erased, &self.tools, prompt));
            while let Some(event) = stream.next().await {
                let event = event.map_err(|err| err.to_string())?;
                if let telos_agent::TurnEvent::TurnFinished { final_text: text, .. } = &event {
                    final_text = text.clone();
                }
                let desktop_event = map_turn_event(event);
                if desktop_event.kind != "ignored" {
                    on_event(desktop_event);
                }
            }
        }
        self.session.save().await.map_err(|err| err.to_string())?;
        Ok(final_text)
    }
}

fn build_provider(settings: &ChatSettings) -> Result<Arc<dyn ModelProvider>, String> {
    match settings.provider {
        ProviderKind::Mock => Ok(Arc::new(MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("Mock desktop response."),
            stop_reason: StopReason::EndTurn,
            usage: None,
        }]))),
        ProviderKind::Deepseek => {
            let api_key = settings
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .ok_or_else(|| "DeepSeek API key is required".to_string())?;
            let model = settings.model.as_deref().unwrap_or("auto").trim();
            if model.is_empty() || model.eq_ignore_ascii_case("auto") {
                Ok(Arc::new(RoutedProvider::new(RoutedModelConfig::dual(
                    api_key.to_string(),
                    "deepseek-v4-pro".to_string(),
                    "deepseek-v4-flash".to_string(),
                ))))
            } else {
                Ok(Arc::new(DeepSeekProvider::new(DeepSeekConfig::new(api_key, model))))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_host_runs_prompt_and_emits_finish_event() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let mut host = AgentHost::new(ChatSettings {
            provider: ProviderKind::Mock,
            cwd: Some(temp.path().to_path_buf()),
            ..ChatSettings::default()
        })
        .expect("mock host should initialize");

        let mut events = Vec::new();
        let final_text = host
            .run_prompt("hello".to_string(), |event| events.push(event))
            .await
            .expect("mock prompt should run");

        assert_eq!(final_text, "Mock desktop response.");
        assert!(events.iter().any(|event| event.kind == "turn_finished"));
    }
}
