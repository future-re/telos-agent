use tracing::{info, warn};

use crate::agent::compaction::estimate_message_tokens;
use crate::agent::context::Conversation;
use crate::agent::turn::TurnEvent;
use crate::error::AgentError;
use crate::model::provider::ModelProvider;

use super::super::{session::SessionInfo, state::RuntimeState};
const MAX_CONSECUTIVE_COMPACTION_FAILURES: usize = 3;

pub(super) async fn compact_if_needed<P>(
    session: &mut SessionInfo,
    context: &mut Conversation,
    state: &mut RuntimeState,
    provider: &P,
    iteration: usize,
) -> Result<bool, AgentError>
where
    P: ModelProvider,
{
    let mut compacted = false;

    // Circuit breaker: skip compaction after repeated failures.
    if state.compaction_failures() >= MAX_CONSECUTIVE_COMPACTION_FAILURES {
        info!(
            iteration,
            failures = state.compaction_failures(),
            "compaction circuit breaker open — skipping compaction this iteration"
        );
        return Ok(compacted);
    }

    if let Some(budget) = session.config().token_budget {
        let estimated_tokens = estimate_message_tokens(context.messages(), provider);
        // Hard cap: abort the turn if we are already over budget.
        if estimated_tokens > budget.max_tokens {
            warn!(
                used_tokens = estimated_tokens,
                max_tokens = budget.max_tokens,
                "token budget exceeded"
            );
            let event = TurnEvent::TokenBudgetExceeded {
                used_tokens: estimated_tokens,
                max_tokens: budget.max_tokens,
            };
            session.emit_turn_event(&event);
            return Err(AgentError::TokenBudgetExceeded {
                used_tokens: estimated_tokens,
                max_tokens: budget.max_tokens,
            });
        }
        // Soft threshold: compact to stay within budget.
        if estimated_tokens >= budget.compact_at_tokens
            && let Some(compaction) = session.config().compaction.clone()
        {
            // Persist a snapshot before mutating messages, so recovery is possible.
            let _ = super::super::session::persistence::save_pre_compact_snapshot_with_events(
                session,
                context.messages(),
                "pre_compact:token_budget",
            )
            .await;
            let started = TurnEvent::CompactionStarted { reason: "token_budget".into() };
            session.emit_turn_event(&started);
            match compaction.compact(context.messages_mut(), provider).await {
                Ok(true) => {
                    compacted = true;
                    state.set_compaction_failures(0);
                    info!(iteration, "token-budget compaction applied");
                }
                // Compact returned Ok(false) → nothing to do.
                Ok(false) => {}
                Err(e) => {
                    state.set_compaction_failures(state.compaction_failures() + 1);
                    warn!(
                        iteration,
                        failures = state.compaction_failures(),
                        error = %e,
                        "compaction failed"
                    );
                    session.emit_turn_event(&TurnEvent::CompactionFailed {
                        reason: "token_budget".into(),
                        error: e.to_string(),
                    });
                    return Err(e);
                }
            }
            let completed = TurnEvent::CompactionCompleted { reason: "token_budget".into() };
            session.emit_turn_event(&completed);
        }
    }

    Ok(compacted)
}
