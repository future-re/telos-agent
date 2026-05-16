use async_trait::async_trait;

use crate::error::AgentError;
use crate::message::{ContentBlock, Message, Role};
use crate::provider::{CompletionRequest, ModelProvider};

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

#[derive(Debug)]
pub struct SummaryCompaction {
    pub max_tokens: usize,
    pub keep_recent: usize,
}

impl Default for SummaryCompaction {
    fn default() -> Self {
        Self {
            max_tokens: 20_000,
            keep_recent: 6,
        }
    }
}

#[async_trait]
impl CompactionStrategy for SummaryCompaction {
    async fn compact(
        &self,
        messages: &mut Vec<Message>,
        provider: &dyn ModelProvider,
    ) -> Result<bool, AgentError> {
        let total_tokens: usize = messages
            .iter()
            .map(|m| {
                m.blocks
                    .iter()
                    .map(|b| match b {
                        ContentBlock::Text(t) => provider.estimate_tokens(&t.text),
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

        // Find the system prompt index (if any)
        let system_idx = messages
            .iter()
            .position(|m| m.role == Role::System)
            .map(|i| i + 1)
            .unwrap_or(0);

        // Split point: keep system prompt + most recent keep_recent messages
        let split_point = messages.len().saturating_sub(self.keep_recent);
        let split_point = split_point.max(system_idx);

        if split_point == 0 {
            return Ok(false);
        }

        let to_summarize = messages[..split_point].to_vec();

        let summary_request = CompletionRequest {
            system_prompt: Some(
                "Summarize the following conversation history concisely, preserving key facts, decisions, and context.".into(),
            ),
            messages: to_summarize,
            tools: vec![],
        };

        let response = provider.complete(summary_request).await?;
        let summary_text = response.message.text_content();

        let summary_msg = Message::system(format!(
            "<conversation_summary>\n{}\n</conversation_summary>",
            summary_text
        ));

        let mut new_messages = vec![summary_msg];
        new_messages.extend(messages[split_point..].iter().cloned());
        *messages = new_messages;

        Ok(true)
    }
}
