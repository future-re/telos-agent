//! Context-level compaction: summarises old messages to keep token usage under budget.
//!
//! Implementations of [`CompactionStrategy`] decide *whether* and *how* to
//! shorten the conversation. The default [`SummaryCompaction`] asks the model
//! to produce a natural-language summary of older turns, then replaces those
//! turns with a single synthetic system message.

use async_trait::async_trait;

use crate::error::AgentError;
use crate::message::{ContentBlock, Message, Role};
use crate::provider::{CompletionRequest, ModelProvider};

/// Strategy for compacting conversation history when tokens exceed a budget.
#[async_trait]
pub trait CompactionStrategy: Send + Sync + std::fmt::Debug {
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
pub struct SummaryCompaction {
    /// Token ceiling — if estimated usage stays under this, no compaction happens.
    pub max_tokens: usize,
    /// How many most-recent messages to keep verbatim. Everything older may be summarised.
    pub keep_recent: usize,
}

impl Default for SummaryCompaction {
    fn default() -> Self {
        Self { max_tokens: 20_000, keep_recent: 6 }
    }
}

#[async_trait]
impl CompactionStrategy for SummaryCompaction {
    async fn compact(
        &self,
        messages: &mut Vec<Message>,
        provider: &dyn ModelProvider,
    ) -> Result<bool, AgentError> {
        // Estimate tokens block-by-block — each block kind serialises differently.
        let total_tokens: usize = messages
            .iter()
            .map(|m| {
                m.blocks
                    .iter()
                    .map(|b| match b {
                        ContentBlock::Text(t) => provider.estimate_tokens(&t.text),
                        ContentBlock::Thinking(thinking) => {
                            provider.estimate_tokens(&thinking.text)
                        }
                        ContentBlock::ToolCall(c) => {
                            provider.estimate_tokens(&c.name)
                                + provider.estimate_tokens(&c.arguments.to_string())
                        }
                        ContentBlock::ToolResult(r) => {
                            provider.estimate_tokens(&r.content.to_string())
                        }
                    })
                    .sum::<usize>()
            })
            .sum();

        if total_tokens <= self.max_tokens {
            return Ok(false);
        }

        // Preserve any leading system prompt — we splice the summary in *after* it.
        let system_idx =
            messages.iter().position(|m| m.role == Role::System).map(|i| i + 1).unwrap_or(0);

        // Split point: everything before this is summarised; everything after is kept.
        // Clamp so we never summarise into (or past) the system prompt.
        let split_point = messages.len().saturating_sub(self.keep_recent);
        let split_point = split_point.max(system_idx);

        if split_point == 0 {
            // Nothing to summarise — the whole history is "recent" already.
            return Ok(false);
        }

        // If the only messages before the split point are the leading system
        // prompt(s), summarising them would not reduce tokens (we keep them and
        // add a summary). Skip compaction in that case to avoid pointless loops.
        if split_point == system_idx {
            return Ok(false);
        }

        let to_summarize = messages[..split_point].to_vec();

        let summary_request = CompletionRequest {
            system_prompt: Some(
                "Summarize the following conversation history concisely, preserving key facts, decisions, and context.".into(),
            ),
            system_prompt_blocks: None,
            messages: to_summarize,
            tools: vec![],
        };

        let response = provider.complete(summary_request).await?;
        let summary_text = response.message.text_content();

        // Wrap the summary in an identifiable XML-ish tag so subsequent
        // inspection can locate it without parsing the model's prose.
        let summary_msg = Message::system(format!(
            "<conversation_summary>\n{}\n</conversation_summary>",
            summary_text
        ));

        // Preserve any leading system prompt(s) before the summary, then keep
        // the recent messages that were not summarised.
        let mut new_messages = messages[..system_idx].to_vec();
        new_messages.push(summary_msg);
        new_messages.extend(messages[split_point..].iter().cloned());
        *messages = new_messages;

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            })
        }

        fn estimate_tokens(&self, text: &str) -> usize {
            text.len()
        }
    }

    #[tokio::test]
    async fn preserves_leading_system_prompt() {
        let compaction = SummaryCompaction { max_tokens: 10, keep_recent: 1 };
        let mut messages =
            vec![Message::system("persona"), Message::user("first"), Message::user("second")];
        let changed = compaction.compact(&mut messages, &FakeProvider).await.unwrap();
        assert!(changed);
        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[0].text_content(), "persona");
        assert!(messages[1].text_content().contains("summary text"));
    }

    #[tokio::test]
    async fn skips_compaction_when_only_system_prompt_is_old() {
        let compaction = SummaryCompaction { max_tokens: 5, keep_recent: 1 };
        let mut messages = vec![Message::system("long system prompt text"), Message::user("hi")];
        let changed = compaction.compact(&mut messages, &FakeProvider).await.unwrap();
        assert!(!changed);
    }
}
