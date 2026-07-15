use std::sync::Arc;

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::event_channel::EventChannel;
use crate::message::{Message, Role};
use crate::metrics::SessionMetrics;
use crate::storage::{SessionMetadata, Storage};
use crate::tool::FileReadState;

use super::info::SessionInfo;

pub async fn save(
    session_id: &str,
    config: &AgentConfig,
    messages: &[Message],
    metrics: &SessionMetrics,
    read_file_state: &FileReadState,
) -> Result<(), AgentError> {
    if let Some(storage) = &config.storage {
        storage.save_snapshot(session_id, messages).await?;
        let state = read_file_state.lock().await.clone();
        let metadata = SessionMetadata {
            next_turn_id: 0, // caller should fill this
            total_input_tokens: metrics.total_input_tokens(),
            total_output_tokens: metrics.total_output_tokens(),
            total_prompt_cache_hit_tokens: metrics.total_prompt_cache_hit_tokens(),
            total_prompt_cache_miss_tokens: metrics.total_prompt_cache_miss_tokens(),
            total_tool_calls: metrics.total_tool_calls(),
            total_tool_errors: metrics.total_tool_errors(),
            total_iterations: metrics.total_iterations(),
            compaction_count: metrics.compaction_count(),
            turn_count: metrics.turn_count(),
            retry_count: metrics.retry_count(),
            read_file_state: state,
        };
        storage.save_metadata(session_id, &metadata).await?;
    }
    Ok(())
}

pub async fn save_pre_compact_snapshot(
    session_id: &str,
    config: &AgentConfig,
    messages: &[Message],
) -> Result<(), AgentError> {
    if let Some(storage) = &config.storage {
        let snapshot_id = format!("{}-pre-compact", session_id);
        storage.save_snapshot(&snapshot_id, messages).await
    } else {
        Ok(())
    }
}

pub async fn resume(
    session_id: impl Into<String>,
    mut config: AgentConfig,
    storage: Arc<dyn Storage>,
) -> Result<(SessionInfo, Vec<Message>), AgentError> {
    config.validate()?;
    let session_id_str = session_id.into();
    let mut messages = storage.load(&session_id_str).await?;
    reconcile_system_prompt(&config, &mut messages);

    let metadata = storage.load_metadata(&session_id_str).await?;
    let next_turn_id = metadata.as_ref().map(|m| m.next_turn_id).unwrap_or(1);

    config.storage = Some(storage);

    let event_channel = if let Some(ref ec_config) = config.event_channel {
        EventChannel::start(ec_config.clone())?
    } else {
        None
    };

    Ok((
        SessionInfo::with_id(session_id_str, config, next_turn_id, event_channel),
        messages,
    ))
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
