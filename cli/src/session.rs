use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// A single message in a chat history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

/// A complete chat history, suitable for save/load to JSON.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatHistory {
    pub messages: Vec<ChatMessage>,
}

impl ChatHistory {
    /// Add a user message with the current timestamp.
    pub fn add_user(&mut self, content: impl Into<String>) {
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: content.into(),
            timestamp: current_timestamp(),
        });
    }

    /// Add an assistant message with the current timestamp.
    pub fn add_assistant(&mut self, content: impl Into<String>) {
        self.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: content.into(),
            timestamp: current_timestamp(),
        });
    }

    /// Serialize the chat history to a JSON file.
    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Deserialize a chat history from a JSON file.
    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let history: ChatHistory = serde_json::from_str(&json)?;
        Ok(history)
    }
}

/// Return the session directory.
///
/// If `project_root` is `Some`, returns `<project_root>/.telos/sessions/`.
/// If `None`, returns `dirs::data_dir().join("telos").join("sessions")`.
pub fn sessions_dir(project_root: Option<&Path>) -> PathBuf {
    match project_root {
        Some(root) => root.join(".telos").join("sessions"),
        None => {
            let base = dirs::data_dir().expect("could not find data directory");
            base.join("telos").join("sessions")
        }
    }
}

/// Generate a unique session filename: `<prefix>-<unix_timestamp>.json`.
///
/// The timestamp is the current unix epoch in seconds.
pub fn next_session_name(_dir: &Path, prefix: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_secs();
    format!("{prefix}-{ts}.json")
}

fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_secs()
        .to_string()
}

/// Manages session filenames and directories.
pub struct SessionManager {
    sessions_dir: PathBuf,
    current: String,
}

impl SessionManager {
    pub fn new(project_root: Option<&Path>) -> Self {
        let sessions_dir = sessions_dir(project_root);
        let current = next_session_name(&sessions_dir, "chat");
        Self { sessions_dir, current }
    }

    pub fn current_name(&self) -> &str {
        &self.current
    }

    pub fn new_session(&mut self) {
        self.current = next_session_name(&self.sessions_dir, "chat");
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions_dir
    }
}
