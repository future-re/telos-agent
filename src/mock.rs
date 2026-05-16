use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Mutex;

use crate::error::AgentError;
use crate::provider::{CompletionRequest, CompletionResponse, ModelProvider};

pub struct MockProvider {
    responses: Mutex<VecDeque<CompletionResponse>>,
    pub requests: Mutex<Vec<CompletionRequest>>,
}

impl MockProvider {
    pub fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ModelProvider for MockProvider {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, AgentError> {
        self.requests.lock().unwrap().push(request);
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| AgentError::Provider("mock provider has no more responses".into()))
    }
}
