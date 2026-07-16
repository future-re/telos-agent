//! The [`ModelProvider`] trait and the [`ErasedProvider`] helper.

use async_stream::try_stream;
use async_trait::async_trait;
use futures_core::stream::Stream;

use crate::error::AgentError;

use super::types::{CompletionRequest, CompletionResponse, ProviderEvent};

/// Abstract LLM backend. Implement this to plug in a new model provider.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Issue a single non-streaming completion request.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError>;

    /// Stream a completion as a sequence of [`ProviderEvent`]s.
    ///
    /// The default implementation calls [`complete`](Self::complete) and
    /// re-emits the result as one synthetic stream — providers that genuinely
    /// stream should override this for incremental delivery.
    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        Box::pin(try_stream! {
            let response = self.complete(request).await?;
            yield ProviderEvent::MessageStart;
            for block in &response.message.blocks {
                match block {
                    crate::model::message::ContentBlock::Text(text) => {
                        yield ProviderEvent::TextDelta(text.text.clone());
                    }
                    crate::model::message::ContentBlock::Thinking(thinking) => {
                        yield ProviderEvent::ThinkingDelta(thinking.text.clone());
                    }
                    crate::model::message::ContentBlock::ToolCall(call) => {
                        yield ProviderEvent::ToolCall(call.clone());
                    }
                    crate::model::message::ContentBlock::ToolResult(_) => {}
                }
            }
            yield ProviderEvent::MessageStop {
                stop_reason: response.stop_reason,
                usage: response.usage,
                model: response.model,
            };
        })
    }

    /// Return the maximum number of tokens that can be requested from this provider.
    ///
    /// The default implementation returns 128,000 tokens, which is the maximum
    /// supported by the `cl100k_base` tokenizer used by most providers. Providers
    /// with a different tokenizer (e.g. Gemini's SentencePiece) can override this.
    fn max_tokens(&self) -> u32 {
        128_000
    }

    /// Estimate the number of tokens for the given text.
    ///
    /// The default implementation uses the `cl100k_base` tokenizer (via
    /// `tiktoken-rs`). Since DeepSeek, Kimi, and most OpenAI-compatible
    /// providers all use cl100k_base-compatible BPE tokenizers, this gives
    /// ±5% accuracy across all built-in providers.
    ///
    /// Providers with a different tokenizer (e.g. Gemini's SentencePiece)
    /// can override this.
    fn estimate_tokens(&self, text: &str) -> usize {
        crate::model::tokens::count_tokens(text)
    }
}

/// A newtype that implements [`ModelProvider`] by delegating to an erased
/// `&dyn ModelProvider` reference. This is useful when adapting an existing
/// borrowed provider to APIs that require a concrete [`ModelProvider`] type.
pub struct ErasedProvider<'a>(pub &'a (dyn ModelProvider + 'a));

#[async_trait]
impl ModelProvider for ErasedProvider<'_> {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError> {
        self.0.complete(request).await
    }

    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        self.0.stream_complete(request)
    }

    fn max_tokens(&self) -> u32 {
        self.0.max_tokens()
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        self.0.estimate_tokens(text)
    }
}
