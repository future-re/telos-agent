use tracing::{info, warn};

use crate::error::AgentError;
use crate::message::ContentBlock;
use crate::provider::ModelProvider;
use crate::runtime::{AgentSession, TurnEvent};

pub(super) enum CompactionResult {
    /// Compaction completed (or was skipped); caller should continue the turn.
    Continue { events: Vec<TurnEvent>, compactions: usize },
    /// Token budget was already exceeded; caller should finish the turn early.
    AbortTurn { events: Vec<TurnEvent> },
}

impl AgentSession {
    /// Run token-budget and general compaction passes for the current iteration.
    ///
    /// Returns the events that should be yielded and the number of compactions
    /// that actually modified the conversation.
    pub(super) async fn run_compaction_phase<P: ModelProvider>(
        &mut self,
        provider: &P,
        iteration: usize,
    ) -> Result<CompactionResult, AgentError> {
        let mut events = Vec::new();
        let mut compactions = 0;

        if let Some(budget) = self.config.token_budget {
            let estimated_tokens = estimate_message_tokens(&self.messages, provider);
            if estimated_tokens > budget.max_tokens {
                warn!(
                    used_tokens = estimated_tokens,
                    max_tokens = budget.max_tokens,
                    "token budget exceeded"
                );
                events.push(TurnEvent::TokenBudgetExceeded {
                    used_tokens: estimated_tokens,
                    max_tokens: budget.max_tokens,
                });
                return Ok(CompactionResult::AbortTurn { events });
            }
            if estimated_tokens >= budget.compact_at_tokens
                && let Some(compaction) = self.config.compaction.clone()
            {
                events.push(TurnEvent::CompactionStarted { reason: "token_budget".into() });
                let did_compact = compaction.compact(&mut self.messages, provider).await?;
                events.push(TurnEvent::CompactionCompleted { reason: "token_budget".into() });
                if did_compact {
                    compactions += 1;
                    self.push_system_reminder(crate::message::SystemReminder::Compaction {
                        reason: "token_budget".into(),
                    });
                    info!(iteration, "token-budget compaction applied");
                }
            }
        }

        if let Some(compaction) = self.config.compaction.clone() {
            events.push(TurnEvent::CompactionStarted { reason: "char_budget".into() });
            let did_compact = compaction.compact(&mut self.messages, provider).await?;
            events.push(TurnEvent::CompactionCompleted { reason: "char_budget".into() });
            if did_compact {
                compactions += 1;
                self.push_system_reminder(crate::message::SystemReminder::Compaction {
                    reason: "char_budget".into(),
                });
                info!(iteration, "char-budget compaction applied");
            }
        }

        Ok(CompactionResult::Continue { events, compactions })
    }
}

/// Sum estimated token counts across every block in `messages`.
///
/// Used by the turn loop to decide whether to invoke compaction or abort the
/// turn before issuing a request the model can't accept.
fn estimate_message_tokens(
    messages: &[crate::message::Message],
    provider: &dyn ModelProvider,
) -> usize {
    messages
        .iter()
        .flat_map(|message| message.blocks.iter())
        .map(|block| match block {
            ContentBlock::Text(text) => provider.estimate_tokens(&text.text),
            ContentBlock::Thinking(thinking) => provider.estimate_tokens(&thinking.text),
            ContentBlock::ToolCall(call) => {
                provider.estimate_tokens(&call.name)
                    + provider.estimate_tokens(&call.arguments.to_string())
            }
            ContentBlock::ToolResult(result) => {
                provider.estimate_tokens(&result.content.to_string())
            }
        })
        .sum()
}
