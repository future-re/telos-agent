use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agent::turn::TurnEvent;
use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::integrations::event_channel::EventChannel;

static NEXT_SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(crate) struct SessionInfo {
    pub(crate) config: AgentConfig,
    pub(crate) session_id: String,
    pub(crate) next_turn_id: u64,
    pub(crate) event_channel: Option<EventChannel>,
    pub(crate) turn_event_sender: Option<tokio::sync::mpsc::UnboundedSender<TurnEvent>>,
    pub(crate) turn_event_log: Option<std::sync::Arc<std::sync::Mutex<Vec<TurnEvent>>>>,
}

impl SessionInfo {
    pub fn new(config: AgentConfig) -> Result<Self, AgentError> {
        config.validate()?;

        let event_channel = if let Some(ref ec_config) = config.event_channel {
            EventChannel::start(ec_config.clone())?
        } else {
            None
        };

        Ok(Self {
            config,
            session_id: new_session_id(),
            next_turn_id: 1,
            event_channel,
            turn_event_sender: None,
            turn_event_log: None,
        })
    }

    pub fn with_id(
        session_id: String,
        config: AgentConfig,
        next_turn_id: u64,
        event_channel: Option<EventChannel>,
    ) -> Self {
        Self {
            config,
            session_id,
            next_turn_id,
            event_channel,
            turn_event_sender: None,
            turn_event_log: None,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut AgentConfig {
        &mut self.config
    }

    pub fn next_turn_id(&self) -> u64 {
        self.next_turn_id
    }
}

impl SessionInfo {
    pub(crate) fn advance_turn_id(&mut self) -> u64 {
        let id = self.next_turn_id;
        self.next_turn_id += 1;
        id
    }

    pub(crate) fn event_channel_mut(&mut self) -> &mut Option<EventChannel> {
        &mut self.event_channel
    }

    pub(crate) fn emit_turn_event(&self, event: &TurnEvent) {
        if let Some(sender) = &self.turn_event_sender {
            let _ = sender.send(event.clone());
        }
        if let Some(log) = &self.turn_event_log
            && let Ok(mut log) = log.lock()
        {
            log.push(event.clone());
        }
        if let Some(channel) = &self.event_channel {
            channel.publish(event);
        }
    }
}

fn new_session_id() -> String {
    let timestamp_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let process_id = std::process::id();
    let sequence = NEXT_SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("session-{timestamp_ns}-{process_id}-{sequence}")
}
