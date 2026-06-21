use std::collections::HashMap;
use std::sync::Arc;

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::message::{Message, Role};
use crate::metrics::SessionMetrics;
use crate::runtime::AgentSession;
use crate::storage::{SessionMetadata, Storage};

impl AgentSession {
    /// Persist the conversation and session metadata if a [`Storage`] backend is configured.
    pub async fn save(&self) -> Result<(), AgentError> {
        if let Some(storage) = &self.config.storage {
            storage.save_snapshot(&self.session_id, &self.messages).await?;
            let read_file_state = self.read_file_state.lock().await.clone();
            let metadata = SessionMetadata {
                next_turn_id: self.next_turn_id,
                total_input_tokens: self.metrics.total_input_tokens(),
                total_output_tokens: self.metrics.total_output_tokens(),
                total_prompt_cache_hit_tokens: self.metrics.total_prompt_cache_hit_tokens(),
                total_prompt_cache_miss_tokens: self.metrics.total_prompt_cache_miss_tokens(),
                total_tool_calls: self.metrics.total_tool_calls(),
                total_tool_errors: self.metrics.total_tool_errors(),
                total_iterations: self.metrics.total_iterations(),
                compaction_count: self.metrics.compaction_count(),
                turn_count: self.metrics.turn_count(),
                retry_count: self.metrics.retry_count(),
                read_file_state,
            };
            storage.save_metadata(&self.session_id, &metadata).await?;
        }
        Ok(())
    }

    /// Resume a previously persisted session from `storage`.
    ///
    /// If the loaded transcript has a different system prompt than `config`,
    /// the config's prompt wins, so the session behaves consistently across
    /// restarts.
    pub async fn resume(
        session_id: impl Into<String>,
        mut config: AgentConfig,
        storage: Arc<dyn Storage>,
    ) -> Result<Self, AgentError> {
        config.validate()?;
        let session_id = session_id.into();
        let mut messages = storage.load(&session_id).await?;
        reconcile_system_prompt(&config, &mut messages);

        let metadata = storage.load_metadata(&session_id).await?;
        let (next_turn_id, metrics, read_file_state) = if let Some(m) = metadata {
            (
                m.next_turn_id,
                SessionMetrics::with_values(
                    m.total_input_tokens,
                    m.total_output_tokens,
                    m.total_prompt_cache_hit_tokens,
                    m.total_prompt_cache_miss_tokens,
                    m.total_tool_calls,
                    m.total_tool_errors,
                    m.total_iterations,
                    m.compaction_count,
                    m.turn_count,
                    m.retry_count,
                ),
                m.read_file_state,
            )
        } else {
            (1, SessionMetrics::new(), HashMap::new())
        };

        config.storage = Some(storage);
        Ok(Self {
            config,
            session_id,
            next_turn_id,
            messages,
            read_file_state: Arc::new(tokio::sync::Mutex::new(read_file_state)),
            metrics,
        })
    }
}

fn reconcile_system_prompt(config: &AgentConfig, messages: &mut Vec<Message>) {
    if config.prompt_assembly.is_some() {
        return;
    }

    let Some(config_system) = config.base_system_prompt.as_ref() else {
        return;
    };

    if messages.is_empty() {
        messages.push(Message::system(config_system.clone()));
        return;
    }

    let loaded_system = messages
        .first()
        .filter(|message| message.role == Role::System)
        .map(|message| message.text_content());
    if loaded_system.as_deref() == Some(config_system.as_str()) {
        return;
    }

    if messages.first().map(|message| message.role) == Some(Role::System) {
        messages[0] = Message::system(config_system.clone());
    } else {
        messages.insert(0, Message::system(config_system.clone()));
    }
}
