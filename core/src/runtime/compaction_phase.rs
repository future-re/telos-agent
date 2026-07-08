use tracing::{info, warn};

use crate::compaction::estimate_message_tokens;
use crate::error::AgentError;
use crate::provider::ModelProvider;
use crate::runtime::{AgentSession, TurnEvent};

/// After this many consecutive compaction failures, stop trying for the
/// rest of the session. Prevents API-waste loops when context is irrecoverably
/// over the limit (e.g. a single tool result larger than the compaction budget).
const MAX_CONSECUTIVE_COMPACTION_FAILURES: usize = 3;

pub(super) enum CompactionPhaseResult {
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
    ) -> Result<CompactionPhaseResult, AgentError> {
        let mut events = Vec::new();
        let mut compactions = 0;

        // Circuit breaker: if compaction has repeatedly failed, skip further
        // attempts. Without this, sessions where context is irrecoverably over
        // the limit hammer the API with doomed compaction calls on every turn.
        if self.consecutive_compaction_failures >= MAX_CONSECUTIVE_COMPACTION_FAILURES {
            info!(
                iteration,
                failures = self.consecutive_compaction_failures,
                "compaction circuit breaker open — skipping compaction this iteration"
            );
            return Ok(CompactionPhaseResult::Continue { events, compactions });
        }

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
                return Ok(CompactionPhaseResult::AbortTurn { events });
            }
            if estimated_tokens >= budget.compact_at_tokens
                && let Some(compaction) = self.config.compaction.clone()
            {
                // Save pre-compact snapshot so old messages are not lost.
                let _ = self.save_pre_compact_snapshot().await;
                events.push(TurnEvent::CompactionStarted { reason: "token_budget".into() });
                match compaction.compact(&mut self.messages, provider).await {
                    Ok(true) => {
                        compactions += 1;
                        self.consecutive_compaction_failures = 0;
                        self.push_system_reminder(crate::message::SystemReminder::Compaction {
                            reason: "token_budget".into(),
                        });
                        info!(iteration, "token-budget compaction applied");
                    }
                    Ok(false) => {
                        // No compaction needed — not a failure.
                    }
                    Err(e) => {
                        self.consecutive_compaction_failures += 1;
                        warn!(
                            iteration,
                            failures = self.consecutive_compaction_failures,
                            error = %e,
                            "compaction failed"
                        );
                        events
                            .push(TurnEvent::CompactionCompleted { reason: "token_budget".into() });
                        return Err(e);
                    }
                }
                events.push(TurnEvent::CompactionCompleted { reason: "token_budget".into() });
            }
        }

        // Only run the general compaction pass if the token-budget pass didn't
        // already compact (avoids double-compacting the same messages).
        if compactions == 0
            && let Some(compaction) = self.config.compaction.clone()
        {
            // Save pre-compact snapshot so old messages are not lost.
            let _ = self.save_pre_compact_snapshot().await;
            events.push(TurnEvent::CompactionStarted { reason: "char_budget".into() });
            match compaction.compact(&mut self.messages, provider).await {
                Ok(true) => {
                    compactions += 1;
                    self.consecutive_compaction_failures = 0;
                    self.push_system_reminder(crate::message::SystemReminder::Compaction {
                        reason: "char_budget".into(),
                    });
                    info!(iteration, "char-budget compaction applied");
                }
                Ok(false) => {
                    // No compaction needed — not a failure.
                }
                Err(e) => {
                    self.consecutive_compaction_failures += 1;
                    warn!(
                        iteration,
                        failures = self.consecutive_compaction_failures,
                        error = %e,
                        "compaction failed"
                    );
                    events.push(TurnEvent::CompactionCompleted { reason: "char_budget".into() });
                    return Err(e);
                }
            }
            events.push(TurnEvent::CompactionCompleted { reason: "char_budget".into() });
        }

        Ok(CompactionPhaseResult::Continue { events, compactions })
    }
}
