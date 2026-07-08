//! History-level compaction: summarises old messages to keep token usage under budget.
//!
//! Implementations of [`HistoryCompactionStrategy`] decide whether and how to shorten
//! the conversation. The default [`SummaryHistoryCompaction`] asks the model to
//! summarise older turns in a single pass, then keeps that summary plus the most
//! recent context.

use async_trait::async_trait;

use crate::compaction::estimate_message_tokens;
use crate::error::AgentError;
use crate::message::{ContentBlock, Message, Role, ToolCall};
use crate::prompt::PromptBlock;
use crate::provider::{CompletionRequest, ModelHint, ModelProvider};

const HISTORY_SUMMARY_PROMPT: &str = include_str!("history_summary_prompt.md");

/// Collapse consecutive assistant tool-call + tool-result message pairs into
/// compact user messages. This removes redundant "double" input/output noise
/// so the summarizer receives cleaner, more token-efficient input.
fn collapse_tool_pairs(messages: Vec<Message>) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::new();
    let len = messages.len();
    let mut i = 0;

    while i < len {
        let msg = &messages[i];

        match msg.role {
            Role::Assistant => {
                let tool_calls: Vec<&ToolCall> = msg.tool_calls().collect();
                if tool_calls.is_empty() {
                    result.push(msg.clone());
                    i += 1;
                    continue;
                }

                let assistant_text: String = msg
                    .blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text(t) => Some(t.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                let mut collapsed = Vec::new();
                if !assistant_text.is_empty() {
                    collapsed.push(assistant_text.to_string());
                }

                let mut j = i + 1;
                while j < len && messages[j].role == Role::Tool {
                    for tr in messages[j].tool_results_iter() {
                        let content_len = tr.content.to_string().len();
                        if tr.is_error {
                            collapsed.push(format!(
                                "Used tool `{}` — ERROR ({} chars)",
                                tr.name, content_len
                            ));
                        } else {
                            collapsed
                                .push(format!("Used tool `{}` — {} chars", tr.name, content_len));
                        }
                    }
                    j += 1;
                }

                if collapsed.len() == 1 && assistant_text.is_empty() {
                    result.push(msg.clone());
                } else {
                    result.push(Message::user(collapsed.join("\n")));
                }
                i = j;
            }
            Role::Tool => {
                let parts: Vec<String> = msg
                    .tool_results_iter()
                    .map(|tr| {
                        let content_len = tr.content.to_string().len();
                        if tr.is_error {
                            format!("Used tool `{}` — ERROR ({} chars)", tr.name, content_len)
                        } else {
                            format!("Used tool `{}` — {} chars", tr.name, content_len)
                        }
                    })
                    .collect();
                if !parts.is_empty() {
                    result.push(Message::user(parts.join("\n")));
                }
                i += 1;
            }
            _ => {
                result.push(msg.clone());
                i += 1;
            }
        }
    }

    result
}

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
    pub summary_output_tokens: usize,
}

#[async_trait]
impl HistoryCompactionStrategy for SummaryHistoryCompaction {
    async fn compact(
        &self,
        messages: &mut Vec<Message>,
        provider: &dyn ModelProvider,
    ) -> Result<bool, AgentError> {
        let system_end = messages.iter().take_while(|m| m.role == Role::System).count();

        let split_point = messages.len().saturating_sub(self.keep_recent);
        let split_point = split_point.max(system_end);

        if split_point <= system_end {
            return Ok(false);
        }

        let old_messages = &messages[system_end..split_point];
        let summary_text = self.summarize_pass(old_messages.to_vec(), provider).await?;

        messages.splice(system_end..split_point, [Message::user(summary_text)]);

        Ok(true)
    }
}

impl SummaryHistoryCompaction {
    /// Summarise old messages in a single pass. If the messages exceed the
    /// input budget, only the most recent portion (closest to the split point)
    /// is included; older messages are dropped.
    ///
    /// Tool call + result pairs are collapsed before summarisation to remove
    /// redundant "double" input/output noise.
    async fn summarize_pass(
        &self,
        messages: Vec<Message>,
        provider: &dyn ModelProvider,
    ) -> Result<String, AgentError> {
        let messages = collapse_tool_pairs(messages);
        let prompt_tokens = provider.estimate_tokens(HISTORY_SUMMARY_PROMPT.trim());
        let budget = self
            .max_summary_input_tokens
            .saturating_sub(prompt_tokens)
            .saturating_sub(self.summary_output_tokens)
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
            max_tokens: Some(self.summary_output_tokens.min(u32::MAX as usize) as u32),
        };

        let response = provider.complete(summary_request).await?;
        Ok(response.message.text_content())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockProvider;
    use crate::provider::{CompletionResponse, DeepSeekConfig, DeepSeekProvider, StopReason};

    const TEST_SUMMARY_FIXTURE: &str = include_str!("test_summary.txt");
    const TEST_SUMMARY_OUTPUT: &str = "test_summary_output.txt";

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
    }

    #[tokio::test]
    async fn preserves_leading_system_prompt_without_summarizing_it() {
        let compaction = SummaryHistoryCompaction {
            keep_recent: 1,
            max_summary_input_tokens: 120_000,
            summary_output_tokens: 4_000,
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

    fn real_deepseek_provider() -> Option<DeepSeekProvider> {
        let _ = dotenvy::from_filename("src/provider/.env");
        dotenvy::dotenv().ok();

        let api_key = std::env::var("DEEPSEEK_TEST_KEY")
            .or_else(|_| std::env::var("DEEPSEEK_API_KEY"))
            .ok()?;
        if api_key.is_empty() || api_key == "your_deepseek_api_key_here" {
            return None;
        }

        Some(DeepSeekProvider::new(DeepSeekConfig {
            api_key,
            model: std::env::var("DEEPSEEK_TEST_MODEL").unwrap_or_else(|_| "deepseek-chat".into()),
            base_url: std::env::var("DEEPSEEK_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com".into()),
        }))
    }

    #[tokio::test]
    async fn summarizes_test_summary_fixture_with_real_model() {
        let provider = match real_deepseek_provider() {
            Some(provider) => provider,
            None => {
                eprintln!("SKIP: DEEPSEEK_TEST_KEY or DEEPSEEK_API_KEY not set");
                return;
            }
        };
        assert!(!TEST_SUMMARY_FIXTURE.trim().is_empty());

        let compaction = SummaryHistoryCompaction {
            keep_recent: 1,
            max_summary_input_tokens: 800_000,
            summary_output_tokens: 4_000,
        };
        let recent_message = Message::assistant("Recent turn remains available verbatim.");
        let mut messages = vec![
            Message::system("persona"),
            Message::user(TEST_SUMMARY_FIXTURE),
            recent_message.clone(),
        ];

        let changed = compaction.compact(&mut messages, &provider).await.unwrap();

        assert!(changed);
        assert_eq!(messages[0], Message::system("persona"));
        let summary_text = messages[1].text_content();
        assert!(!summary_text.trim().is_empty());
        assert_ne!(summary_text.trim(), TEST_SUMMARY_FIXTURE.trim());
        let output_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/compaction")
            .join(TEST_SUMMARY_OUTPUT);
        std::fs::write(&output_path, summary_text.trim()).unwrap_or_else(|err| {
            panic!("failed to write summary output to {}: {err}", output_path.display())
        });
        assert_eq!(messages[2], recent_message);
    }

    #[tokio::test]
    async fn skips_compaction_when_only_system_prompt_is_old() {
        let compaction = SummaryHistoryCompaction {
            keep_recent: 1,
            max_summary_input_tokens: 120_000,
            summary_output_tokens: 4_000,
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
            summary_output_tokens: 4_000,
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
            max_summary_input_tokens: prompt_tokens + 1 + 4_000,
            summary_output_tokens: 4_000,
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
            !summarized
                .iter()
                .any(|s| s == "very old message that is quite long and will be dropped"),
            "should drop the oldest message that doesn't fit budget"
        );
    }
}
