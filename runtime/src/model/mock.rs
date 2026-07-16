//! Mock provider for testing — dequeues pre-configured responses.
//!
//! [`MockProvider`] is constructed with a list of [`CompletionResponse`]s.
//! Each call to [`complete`](MockProvider::complete) pops the next one from the
//! queue and records the request it received. Running out of responses returns
//! a provider error — making it easy to assert "the test exercised exactly N
//! turns".

use async_trait::async_trait;
use std::collections::VecDeque;
use tokio::sync::Mutex;

use crate::error::AgentError;
use crate::model::provider::{CompletionRequest, CompletionResponse, ModelProvider};

/// A [`ModelProvider`] that returns pre-configured responses from a queue.
pub struct MockProvider {
    responses: Mutex<VecDeque<CompletionResponse>>,
    /// All requests received, in arrival order. Public so tests can assert on them.
    pub requests: Mutex<Vec<CompletionRequest>>,
}

impl MockProvider {
    /// Build a mock that will reply with `responses` in FIFO order.
    pub fn new(responses: Vec<CompletionResponse>) -> Self {
        Self { responses: Mutex::new(responses.into()), requests: Mutex::new(Vec::new()) }
    }
}

#[async_trait]
impl ModelProvider for MockProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError> {
        self.requests.lock().await.push(request);
        self.responses.lock().await.pop_front().ok_or_else(|| {
            AgentError::Provider(crate::error::ProviderError::Other(
                "mock provider has no more responses".into(),
            ))
        })
    }
}
