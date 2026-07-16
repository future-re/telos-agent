//! Session storage for persisting and resuming agent conversations.
//!
//! The backend is pluggable via the [`Storage`] trait. Two built-in
//! implementations:
//! - [`JsonlStorage`] — one JSON line per message, on disk, under `<dir>/<session_id>.jsonl`.
//! - [`NoopStorage`] — black-hole; useful for tests and ephemeral sessions.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

use crate::error::AgentError;
use crate::model::message::Message;
use crate::tools::api::FileReadRecord;

/// Session metadata persisted alongside messages so a resumed session can
/// restore turn IDs, metrics, and file-read state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionMetadata {
    pub next_turn_id: u64,
    pub total_input_tokens: usize,
    pub total_output_tokens: usize,
    #[serde(default)]
    pub total_prompt_cache_hit_tokens: usize,
    #[serde(default)]
    pub total_prompt_cache_miss_tokens: usize,
    pub total_tool_calls: usize,
    pub total_tool_errors: usize,
    pub total_iterations: usize,
    pub compaction_count: usize,
    pub turn_count: usize,
    pub retry_count: usize,
    pub read_file_state: HashMap<PathBuf, FileReadRecord>,
}

/// Storage backend for persisting agent sessions.
///
/// [`JsonlStorage`] writes one JSON line per message; [`NoopStorage`] discards everything.
#[async_trait]
pub trait Storage: Send + Sync + std::fmt::Debug {
    /// Overwrite the stored conversation with a full snapshot.
    async fn save_snapshot(&self, session_id: &str, messages: &[Message])
    -> Result<(), AgentError>;
    /// Append messages to the existing log (does not truncate).
    async fn append(&self, session_id: &str, messages: &[Message]) -> Result<(), AgentError>;
    /// Load all messages for a session. Returns an empty vec when the session is unknown.
    async fn load(&self, session_id: &str) -> Result<Vec<Message>, AgentError>;
    /// Persist session metadata. The default is a no-op for backwards compatibility.
    async fn save_metadata(
        &self,
        _session_id: &str,
        _metadata: &SessionMetadata,
    ) -> Result<(), AgentError> {
        Ok(())
    }
    /// Load session metadata. The default returns `None` for backwards compatibility.
    async fn load_metadata(
        &self,
        _session_id: &str,
    ) -> Result<Option<SessionMetadata>, AgentError> {
        Ok(None)
    }
    /// Delete all persisted data for a session. The default is a no-op.
    async fn delete(&self, _session_id: &str) -> Result<(), AgentError> {
        Ok(())
    }
}

/// On-disk JSONL backend. Each message is serialised to one line; the file is
/// named `<session_id>.jsonl` inside the configured directory.
#[derive(Debug)]
pub struct JsonlStorage {
    dir: PathBuf,
}

impl JsonlStorage {
    /// Create the storage directory eagerly and return a handle.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self, AgentError> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)
            .map_err(|e| AgentError::Config(format!("failed to create storage directory: {e}")))?;
        Ok(Self { dir })
    }

    /// Path on disk for the given session ID.
    fn path(&self, session_id: &str) -> Result<PathBuf, AgentError> {
        validate_session_id(session_id)?;
        Ok(self.dir.join(format!("{session_id}.jsonl")))
    }

    /// Path on disk for the session metadata file.
    fn metadata_path(&self, session_id: &str) -> Result<PathBuf, AgentError> {
        validate_session_id(session_id)?;
        Ok(self.dir.join(format!("{session_id}.metadata.json")))
    }
}

/// Reject session IDs that could escape the storage directory.
fn validate_session_id(session_id: &str) -> Result<(), AgentError> {
    if session_id.is_empty() {
        return Err(AgentError::Config("session_id cannot be empty".into()));
    }
    if !session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AgentError::Config(format!(
            "session_id contains invalid characters: {session_id}"
        )));
    }
    Ok(())
}

#[async_trait]
impl Storage for JsonlStorage {
    async fn save_snapshot(
        &self,
        session_id: &str,
        messages: &[Message],
    ) -> Result<(), AgentError> {
        tokio::fs::create_dir_all(&self.dir)
            .await
            .map_err(|e| AgentError::Config(format!("failed to create storage directory: {e}")))?;
        let path = self.path(session_id)?;
        // Write to a temporary file first, then atomically rename to the target
        // on the same filesystem. A crash mid-write leaves the original intact.
        let tmp_path = path.with_extension("jsonl.tmp");
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp_path)
            .await
            .map_err(|e| AgentError::Config(format!("storage open failed: {e}")))?;

        let mut writer = BufWriter::new(file);
        for msg in messages {
            let line = serde_json::to_string(msg)
                .map_err(|e| AgentError::Config(format!("serialize failed: {e}")))?;
            writer
                .write_all(line.as_bytes())
                .await
                .map_err(|e| AgentError::Config(format!("storage write failed: {e}")))?;
            writer
                .write_all(b"\n")
                .await
                .map_err(|e| AgentError::Config(format!("storage write failed: {e}")))?;
        }
        writer
            .flush()
            .await
            .map_err(|e| AgentError::Config(format!("storage flush failed: {e}")))?;
        // Atomically rename temp -> target (same-filesystem guarantee on Linux).
        tokio::fs::rename(&tmp_path, &path)
            .await
            .map_err(|e| AgentError::Config(format!("storage rename failed: {e}")))?;
        Ok(())
    }

    async fn append(&self, session_id: &str, messages: &[Message]) -> Result<(), AgentError> {
        tokio::fs::create_dir_all(&self.dir)
            .await
            .map_err(|e| AgentError::Config(format!("failed to create storage directory: {e}")))?;
        let path = self.path(session_id)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .map_err(|e| AgentError::Config(format!("storage open failed: {e}")))?;

        let mut writer = BufWriter::new(file);
        for msg in messages {
            let line = serde_json::to_string(msg)
                .map_err(|e| AgentError::Config(format!("serialize failed: {e}")))?;
            writer
                .write_all(line.as_bytes())
                .await
                .map_err(|e| AgentError::Config(format!("storage write failed: {e}")))?;
            writer
                .write_all(b"\n")
                .await
                .map_err(|e| AgentError::Config(format!("storage write failed: {e}")))?;
        }
        writer
            .flush()
            .await
            .map_err(|e| AgentError::Config(format!("storage flush failed: {e}")))?;
        Ok(())
    }

    async fn load(&self, session_id: &str) -> Result<Vec<Message>, AgentError> {
        let path = self.path(session_id)?;
        if !path.exists() {
            // Unknown session — treat as empty rather than an error so resume() can fall back.
            return Ok(Vec::new());
        }

        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|e| AgentError::Config(format!("storage open failed: {e}")))?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let mut messages = Vec::new();
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| AgentError::Config(format!("storage read failed: {e}")))?
        {
            if line.trim().is_empty() {
                continue;
            }
            let msg: Message = serde_json::from_str(&line)
                .map_err(|e| AgentError::Config(format!("deserialize failed: {e}")))?;
            messages.push(msg);
        }

        Ok(messages)
    }

    async fn save_metadata(
        &self,
        session_id: &str,
        metadata: &SessionMetadata,
    ) -> Result<(), AgentError> {
        tokio::fs::create_dir_all(&self.dir)
            .await
            .map_err(|e| AgentError::Config(format!("failed to create storage directory: {e}")))?;
        let path = self.metadata_path(session_id)?;
        let tmp_path = path.with_extension("metadata.json.tmp");
        let bytes = serde_json::to_vec_pretty(metadata)
            .map_err(|e| AgentError::Config(format!("serialize metadata failed: {e}")))?;
        tokio::fs::write(&tmp_path, bytes)
            .await
            .map_err(|e| AgentError::Config(format!("storage metadata write failed: {e}")))?;
        tokio::fs::rename(&tmp_path, &path)
            .await
            .map_err(|e| AgentError::Config(format!("storage metadata rename failed: {e}")))?;
        Ok(())
    }

    async fn load_metadata(&self, session_id: &str) -> Result<Option<SessionMetadata>, AgentError> {
        let path = self.metadata_path(session_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| AgentError::Config(format!("storage metadata read failed: {e}")))?;
        if bytes.is_empty() {
            return Ok(None);
        }
        let metadata = serde_json::from_slice(&bytes)
            .map_err(|e| AgentError::Config(format!("deserialize metadata failed: {e}")))?;
        Ok(Some(metadata))
    }

    async fn delete(&self, session_id: &str) -> Result<(), AgentError> {
        let jsonl_path = self.path(session_id)?;
        if jsonl_path.exists() {
            tokio::fs::remove_file(&jsonl_path)
                .await
                .map_err(|e| AgentError::Config(format!("storage delete failed: {e}")))?;
        }
        let metadata_path = self.metadata_path(session_id)?;
        if metadata_path.exists() {
            tokio::fs::remove_file(&metadata_path)
                .await
                .map_err(|e| AgentError::Config(format!("storage metadata delete failed: {e}")))?;
        }
        Ok(())
    }
}

/// Storage backend that discards everything — useful for tests / ephemeral sessions.
#[derive(Debug)]
pub struct NoopStorage;

#[async_trait]
impl Storage for NoopStorage {
    async fn save_snapshot(
        &self,
        _session_id: &str,
        _messages: &[Message],
    ) -> Result<(), AgentError> {
        Ok(())
    }

    async fn append(&self, _session_id: &str, _messages: &[Message]) -> Result<(), AgentError> {
        Ok(())
    }

    async fn load(&self, _session_id: &str) -> Result<Vec<Message>, AgentError> {
        Ok(Vec::new())
    }

    async fn save_metadata(
        &self,
        _session_id: &str,
        _metadata: &SessionMetadata,
    ) -> Result<(), AgentError> {
        Ok(())
    }

    async fn load_metadata(
        &self,
        _session_id: &str,
    ) -> Result<Option<SessionMetadata>, AgentError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn jsonl_roundtrip_save_and_load() {
        let dir = std::env::temp_dir().join("tiny_agent_test_storage_roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        let storage = JsonlStorage::new(&dir).unwrap();

        let msgs =
            vec![Message::system("system"), Message::user("hello"), Message::assistant("hi there")];

        storage.save_snapshot("test-session", &msgs).await.unwrap();
        let loaded = storage.load("test-session").await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].text_content(), "system");
        assert_eq!(loaded[1].text_content(), "hello");
        assert_eq!(loaded[2].text_content(), "hi there");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn jsonl_load_unknown_session_returns_empty() {
        let storage =
            JsonlStorage::new(std::env::temp_dir().join("tiny_agent_test_storage_unknown"))
                .unwrap();
        let loaded = storage.load("nonexistent-session").await.unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn jsonl_append_preserves_existing_messages() {
        let dir = std::env::temp_dir().join("tiny_agent_test_storage_append");
        let _ = std::fs::remove_dir_all(&dir);
        let storage = JsonlStorage::new(&dir).unwrap();

        storage.save_snapshot("s", &[Message::user("first")]).await.unwrap();
        storage.append("s", &[Message::assistant("second")]).await.unwrap();
        let loaded = storage.load("s").await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].text_content(), "first"); // snapshot was truncated, so snapshot wrote it
        assert_eq!(loaded[1].text_content(), "second");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn jsonl_snapshot_replaces_content() {
        let dir = std::env::temp_dir().join("tiny_agent_test_storage_snapshot");
        let _ = std::fs::remove_dir_all(&dir);
        let storage = JsonlStorage::new(&dir).unwrap();

        storage.save_snapshot("s", &[Message::user("old")]).await.unwrap();
        storage.save_snapshot("s", &[Message::user("new")]).await.unwrap();
        let loaded = storage.load("s").await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].text_content(), "new");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn noop_storage_always_returns_empty() {
        let storage = NoopStorage;
        storage.save_snapshot("x", &[Message::user("hi")]).await.unwrap();
        let loaded = storage.load("x").await.unwrap();
        assert!(loaded.is_empty());
        storage.append("x", &[Message::user("more")]).await.unwrap();
        let loaded2 = storage.load("x").await.unwrap();
        assert!(loaded2.is_empty());
    }

    #[tokio::test]
    async fn jsonl_rejects_path_traversal_session_id() {
        let dir = std::env::temp_dir().join("tiny_agent_test_storage_path_traversal");
        let _ = std::fs::remove_dir_all(&dir);
        let storage = JsonlStorage::new(&dir).unwrap();

        let result = storage.save_snapshot("../../../etc/evil", &[Message::user("x")]).await;
        assert!(matches!(result, Err(AgentError::Config(_))));

        let result = storage.load("dir/sub").await;
        assert!(matches!(result, Err(AgentError::Config(_))));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn jsonl_metadata_roundtrip() {
        let dir = std::env::temp_dir().join("tiny_agent_test_storage_metadata");
        let _ = std::fs::remove_dir_all(&dir);
        let storage = JsonlStorage::new(&dir).unwrap();

        let mut read_file_state = HashMap::new();
        read_file_state.insert(
            PathBuf::from("src/lib.rs"),
            FileReadRecord {
                content: "fn main() {}".to_string(),
                timestamp_ms: 1234,
                is_partial_view: true,
                offset: Some(0),
                limit: Some(10),
            },
        );

        let metadata = SessionMetadata {
            next_turn_id: 42,
            total_input_tokens: 100,
            total_output_tokens: 50,
            total_prompt_cache_hit_tokens: 10,
            total_prompt_cache_miss_tokens: 90,
            total_tool_calls: 5,
            total_tool_errors: 1,
            total_iterations: 20,
            compaction_count: 2,
            turn_count: 10,
            retry_count: 3,
            read_file_state,
        };

        storage.save_metadata("s", &metadata).await.unwrap();
        let loaded = storage.load_metadata("s").await.unwrap().unwrap();
        assert_eq!(loaded.next_turn_id, 42);
        assert_eq!(loaded.total_input_tokens, 100);
        assert_eq!(loaded.total_prompt_cache_hit_tokens, 10);
        assert_eq!(loaded.total_prompt_cache_miss_tokens, 90);
        assert_eq!(loaded.total_tool_calls, 5);
        let key = PathBuf::from("src/lib.rs");
        assert!(loaded.read_file_state.contains_key(&key));
        assert_eq!(loaded.read_file_state[&key].content, "fn main() {}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn jsonl_metadata_load_unknown_returns_none() {
        let storage = JsonlStorage::new(
            std::env::temp_dir().join("tiny_agent_test_storage_metadata_unknown"),
        )
        .unwrap();
        let loaded = storage.load_metadata("nonexistent-session").await.unwrap();
        assert!(loaded.is_none());
    }
}
