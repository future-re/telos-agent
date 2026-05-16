use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

use crate::error::AgentError;
use crate::message::Message;

#[async_trait]
pub trait Storage: Send + Sync + std::fmt::Debug {
    async fn append(&self, session_id: &str, messages: &[Message]) -> Result<(), AgentError>;
    async fn load(&self, session_id: &str) -> Result<Vec<Message>, AgentError>;
}

#[derive(Debug)]
pub struct JsonlStorage {
    dir: PathBuf,
}

impl JsonlStorage {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self, AgentError> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).map_err(|e| {
            AgentError::Config(format!("failed to create storage directory: {e}"))
        })?;
        Ok(Self { dir })
    }

    fn path(&self, session_id: &str) -> PathBuf {
        self.dir.join(format!("{session_id}.jsonl"))
    }
}

#[async_trait]
impl Storage for JsonlStorage {
    async fn append(&self, session_id: &str, messages: &[Message]) -> Result<(), AgentError> {
        let path = self.path(session_id);
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
        let path = self.path(session_id);
        if !path.exists() {
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
}

#[derive(Debug)]
pub struct NoopStorage;

#[async_trait]
impl Storage for NoopStorage {
    async fn append(&self, _session_id: &str, _messages: &[Message]) -> Result<(), AgentError> {
        Ok(())
    }

    async fn load(&self, _session_id: &str) -> Result<Vec<Message>, AgentError> {
        Ok(Vec::new())
    }
}
