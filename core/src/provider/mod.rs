//! Model provider abstraction — pluggable LLM backends.
//!
//! Built-in backends: [`DeepSeekProvider`].
//! The default [`ModelProvider::stream_complete`] wraps [`ModelProvider::complete`]
//! so non-streaming providers automatically get a (single-chunk) streaming impl.

use async_trait::async_trait;
use futures_core::stream::Stream;

use crate::error::AgentError;

mod openai_compat;

#[cfg(test)]
mod test;

pub mod deepseek;
mod traits;
mod types;

pub use deepseek::{DeepSeekConfig, DeepSeekProvider};
pub use traits::{ErasedProvider, ModelProvider};
pub use types::{CompletionRequest, CompletionResponse, ProviderEvent, StopReason, TokenUsage};

// Implement ModelProvider for reference-to-dyn-trait-object so that
// `run_turn_stream` can accept `&dyn ModelProvider` through `ErasedProvider`.
#[async_trait]
impl ModelProvider for &(dyn ModelProvider + Send + Sync) {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError> {
        (**self).complete(request).await
    }

    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        (**self).stream_complete(request)
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        (**self).estimate_tokens(text)
    }
}

/// Implement [`ModelProvider`] for `Arc<dyn ModelProvider + Send + Sync>` so
/// that code holding a type-erased provider pointer can call
/// [`AgentSession::run_turn_stream`](crate::AgentSession::run_turn_stream)
/// directly via `&arc`.
#[async_trait]
impl ModelProvider for std::sync::Arc<dyn ModelProvider + Send + Sync> {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError> {
        (**self).complete(request).await
    }

    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        (**self).stream_complete(request)
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        (**self).estimate_tokens(text)
    }
}
