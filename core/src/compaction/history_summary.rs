//! History-level compaction: summarises old messages to keep token usage under budget.
//!
//! Implementations of [`HistoryCompactionStrategy`] decide whether and how to shorten
//! the conversation. The default [`SummaryHistoryCompaction`] asks the model to
//! summarise older turns, then keeps that summary plus the most recent context.

use async_trait::async_trait;

use crate::error::AgentError;
use crate::message::{Message, Role};
use crate::prompt::PromptBlock;
use crate::provider::{CompletionRequest, ModelHint, ModelProvider};

const DEFAULT_MAX_TOKENS: usize = 50_000;
const DEFAULT_KEEP_RECENT: usize = 4_000;
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
    /// Token ceiling — if estimated usage stays under this, no compaction happens.
    pub max_tokens: usize,
    /// How many most-recent messages to keep verbatim. Everything older may be summarised.
    pub keep_recent: usize,
}

impl Default for SummaryHistoryCompaction {
    fn default() -> Self {
        Self { max_tokens: DEFAULT_MAX_TOKENS, keep_recent: DEFAULT_KEEP_RECENT }
    }
}

#[async_trait]
impl HistoryCompactionStrategy for SummaryHistoryCompaction {
    async fn compact(
        &self,
        messages: &mut Vec<Message>,
        provider: &dyn ModelProvider,
    ) -> Result<bool, AgentError> {
        let total_tokens = crate::compaction::estimate_message_tokens(messages, provider);

        if total_tokens <= self.max_tokens {
            return Ok(false);
        }

        // Preserve leading system prompts verbatim and splice the summary after them.
        let system_end = messages.iter().take_while(|m| m.role == Role::System).count();

        // Split point: everything before this is summarised; everything after is kept.
        // Clamp so the summary never replaces leading system prompts.
        let split_point = messages.len().saturating_sub(self.keep_recent);
        let split_point = split_point.max(system_end);

        // If the only messages before the split point are leading system
        // prompts, summarising would not reduce tokens because they are always
        // kept verbatim. Skip compaction to avoid pointless loops.
        if split_point <= system_end {
            return Ok(false);
        }

        let to_summarize = messages[system_end..split_point].to_vec();

        let summary_request = CompletionRequest {
            system_prompt_blocks: vec![PromptBlock::dynamic(
                "history_summary",
                HISTORY_SUMMARY_PROMPT.trim(),
            )],
            messages: to_summarize,
            tools: vec![],
            model_hint: Some(ModelHint::Summarization),
            max_tokens: None,
        };

        let response = provider.complete(summary_request).await?;
        let summary_text = response.message.text_content();

        // Wrap the summary in an identifiable XML-ish tag so subsequent
        // inspection can locate it without parsing the model's prose.
        let summary_msg = Message::user(format!(
            "<conversation_summary>\n{}\n</conversation_summary>",
            summary_text
        ));

        // Build the compacted conversation:
        //   1. Leading system prompt (preserved verbatim)
        //   2. Compact boundary marker (so future code can locate the split)
        //   3. Summary of old messages
        //   4. Recent messages (preserved verbatim)
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let boundary_msg = Message::user(format!(
            "<compact_boundary timestamp='{ts}' original_count='{oc}' summary_count='1' keep_recent='{kr}'/>",
            ts = timestamp,
            oc = split_point,
            kr = self.keep_recent,
        ));
        let mut new_messages = messages[..system_end].to_vec();
        new_messages.push(boundary_msg);
        new_messages.push(summary_msg);
        new_messages.extend(messages[split_point..].iter().cloned());
        *messages = new_messages;

        Ok(true)
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
        let compaction = SummaryHistoryCompaction { max_tokens: 1, keep_recent: 1 };
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
        // messages[1] is the compact boundary marker (user role so it
        // doesn't interfere with system prompt caching detection)
        assert_eq!(messages[1].role, Role::User);
        assert!(messages[1].text_content().contains("<compact_boundary"));
        assert!(messages[2].text_content().contains("summary text"));

        let requests = provider.requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages, vec![Message::user("first")]);
    }

    #[tokio::test]
    async fn skips_compaction_when_only_system_prompt_is_old() {
        let compaction = SummaryHistoryCompaction { max_tokens: 5, keep_recent: 1 };
        let mut messages = vec![Message::system("long system prompt text"), Message::user("hi")];
        let changed = compaction.compact(&mut messages, &FakeProvider).await.unwrap();
        assert!(!changed);
    }
}
