//! History-level compaction: summarises old messages to keep token usage under budget.
//!
//! Implementations of [`HistoryCompactionStrategy`] decide whether and how to shorten
//! the conversation. The default [`SummaryHistoryCompaction`] asks the model to
//! summarise older turns in a single pass, then keeps that summary plus the most
//! recent context.

use async_trait::async_trait;

use crate::compaction::estimate_message_tokens;
use crate::error::AgentError;
use crate::message::{Message, Role};
use crate::prompt::PromptBlock;
use crate::provider::{CompletionRequest, ModelHint, ModelProvider};

const HISTORY_SUMMARY_PROMPT: &str = include_str!("history_summary_prompt.md");

/// Strategy for compacting conversation history when tokens exceed a budget.
#[async_trait]
pub trait HistoryCompactionStrategy: Send + Sync + std::fmt::Debug {
    /// Attempt to compact `messages` if they exceed a budget.
    ///
    /// Returns `true` if compaction occurred, `false` otherwise.
    async fn compact(
        &self,
        messages: &mut Vec<Message>,
        provider: &dyn ModelProvider,
    ) -> Result<bool, AgentError>;
}

/// Compacts by asking the model to summarise old messages, keeping the most recent N.
#[derive(Debug)]
pub struct SummaryHistoryCompaction {
    /// How many most-recent messages to keep verbatim. Everything older may be summarised.
    pub keep_recent: usize,
    /// Maximum input budget for the summarisation provider call.
    pub max_summary_input_tokens: usize,
    /// Output budget reserved for the summarisation provider call.
    pub summary_max_output_tokens: usize,
}

#[async_trait]
impl HistoryCompactionStrategy for SummaryHistoryCompaction {
    async fn compact(
        &self,
        messages: &mut Vec<Message>,
        provider: &dyn ModelProvider,
    ) -> Result<bool, AgentError> {
        let total_tokens = crate::compaction::estimate_message_tokens(messages, provider);

        let system_end = messages.iter().take_while(|m| m.role == Role::System).count();

        let split_point = messages.len().saturating_sub(self.keep_recent);
        let split_point = split_point.max(system_end);

        if split_point <= system_end {
            return Ok(false);
        }

        let old_messages = &messages[system_end..split_point];
        let summary_text =
            self.summarize_pass(old_messages.to_vec(), provider, self.keep_recent).await?;

        messages.splice(system_end..split_point, [Message::user(summary_text)]);

        Ok(true)
    }
}

impl SummaryHistoryCompaction {
    /// Summarise old messages in a single pass. If the messages exceed the
    /// input budget, only the most recent portion (closest to the split point)
    /// is included; older messages are dropped.
    async fn summarize_pass(
        &self,
        messages: Vec<Message>,
        provider: &dyn ModelProvider,
        need_keep_recent: usize
    ) -> Result<String, AgentError> {
        let prompt_tokens = provider.estimate_tokens(HISTORY_SUMMARY_PROMPT.trim());
        let need_keep_recent_tokens = estimate_message_tokens(&messages[messages.len().saturating_sub(need_keep_recent)..], provider);
        let budget = self
            .max_summary_input_tokens
            .saturating_sub(prompt_tokens)
            .saturating_sub(self.summary_max_output_tokens).saturating_sub(need_keep_recent_tokens)
            .max(1);

        let mut selected = Vec::new();
        let mut tokens_used = 0usize;

        for msg in messages.into_iter().rev() {
            let msg_tokens = estimate_message_tokens(std::slice::from_ref(&msg), provider);
            if tokens_used + msg_tokens > budget && !selected.is_empty() {
                break;
            }
            tokens_used += msg_tokens;
            selected.push(msg);
        }
        selected.reverse();

        if selected.is_empty() {
            return Ok(String::new());
        }

        self.complete_summary(selected, provider).await
    }

    async fn complete_summary(
        &self,
        messages: Vec<Message>,
        provider: &dyn ModelProvider,
    ) -> Result<String, AgentError> {
        let summary_request = CompletionRequest {
            system_prompt_blocks: vec![PromptBlock::dynamic(
                "history_summary",
                HISTORY_SUMMARY_PROMPT.trim(),
            )],
            messages,
            tools: vec![],
            model_hint: Some(ModelHint::Summarization),
            max_tokens: Some(self.summary_max_output_tokens.min(u32::MAX as usize) as u32),
        };

        let response = provider.complete(summary_request).await?;
        Ok(response.message.text_content())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockProvider;
    use crate::provider::{CompletionResponse, StopReason};

    struct FakeProvider;

    #[async_trait::async_trait]
    impl ModelProvider for FakeProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, AgentError> {
            Ok(CompletionResponse {
                message: Message::assistant("summary text"),
                stop_reason: StopReason::EndTurn,
                usage: None,
                model: None,
            })
        }

        fn estimate_tokens(&self, text: &str) -> usize {
            text.len()
        }
    }

    #[tokio::test]
    async fn preserves_leading_system_prompt_without_summarizing_it() {
        let compaction = SummaryHistoryCompaction {
            keep_recent: 1,
            max_summary_input_tokens: 120_000,
            summary_max_output_tokens: 4_000,
        };
        let mut messages =
            vec![Message::system("persona"), Message::user("first"), Message::user("second")];
        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("summary text"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        }]);

        let changed = compaction.compact(&mut messages, &provider).await.unwrap();

        assert!(changed);
        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[0].text_content(), "persona");
        assert_eq!(messages[1].role, Role::User);
        assert!(messages[1].text_content().contains("summary text"));

        let requests = provider.requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages, vec![Message::user("first")]);
    }

    #[tokio::test]
    async fn skips_compaction_when_only_system_prompt_is_old() {
        let compaction = SummaryHistoryCompaction {
            keep_recent: 1,
            max_summary_input_tokens: 120_000,
            summary_max_output_tokens: 4_000,
        };
        let mut messages = vec![Message::system("long system prompt text"), Message::user("hi")];
        let changed = compaction.compact(&mut messages, &FakeProvider).await.unwrap();
        assert!(!changed);
    }

    #[tokio::test]
    async fn summarizes_old_messages_in_one_pass() {
        let compaction = SummaryHistoryCompaction {
            keep_recent: 1,
            max_summary_input_tokens: 120_000,
            summary_max_output_tokens: 4_000,
        };
        let mut messages = vec![
            Message::system("persona"),
            Message::user("early chat"),
            Message::user("some history"),
            Message::user("recent"),
        ];
        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("condensed summary"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        }]);

        let changed = compaction.compact(&mut messages, &provider).await.unwrap();

        assert!(changed);
        assert!(messages[1].text_content().contains("condensed summary"));

        let requests = provider.requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].messages,
            vec![Message::user("early chat"), Message::user("some history")]
        );
    }

    #[tokio::test]
    async fn drops_oldest_messages_when_exceeding_summary_input_budget() {
        let prompt_tokens = FakeProvider.estimate_tokens(HISTORY_SUMMARY_PROMPT.trim());
        let compaction = SummaryHistoryCompaction {
            keep_recent: 1,
            max_summary_input_tokens: prompt_tokens + 50 + 4_000,
            summary_max_output_tokens: 4_000,
        };
        let mut messages = vec![
            Message::system("persona"),
            Message::user("very old message that is quite long and will be dropped"),
            Message::user("more recent old message to keep"),
            Message::user("recent"),
        ];
        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("partial summary"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        }]);

        let changed = compaction.compact(&mut messages, &provider).await.unwrap();
        assert!(changed);

        let requests = provider.requests.lock().await;
        assert_eq!(requests.len(), 1);
        let summarized: Vec<String> =
            requests[0].messages.iter().map(|m| m.text_content()).collect();
        assert!(
            summarized.iter().any(|s| s.contains("more recent old message")),
            "should keep the more recent old message"
        );
        assert!(
            !summarized.iter().any(|s| s == "very old message that is quite long and will be dropped"),
            "should drop the oldest message that doesn't fit budget"
        );
    }
}
