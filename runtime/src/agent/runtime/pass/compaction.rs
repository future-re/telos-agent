use tracing::{info, warn};

use crate::agent::compaction::estimate_message_tokens;
use crate::agent::context::Conversation;
use crate::agent::turn::TurnEvent;
use crate::error::AgentError;
use crate::model::provider::ModelProvider;

use super::super::{session::SessionInfo, state::RuntimeState};
use super::MAX_CONSECUTIVE_COMPACTION_FAILURES;

/// Outcome of the compaction phase.
pub(super) enum CompactionPhaseResult {
    /// Proceed with the current turn, possibly with compaction events recorded.
    Continue { events: Vec<TurnEvent>, compactions: usize },
    /// The turn must be aborted (e.g., token budget is already exceeded).
    AbortTurn { events: Vec<TurnEvent> },
}

pub(super) async fn run_compaction_phase<P>(
    session: &mut SessionInfo,
    context: &mut Conversation,
    state: &mut RuntimeState,
    provider: &P,
    iteration: usize,
) -> Result<CompactionPhaseResult, AgentError>
where
    P: ModelProvider,
{
    let mut events = Vec::new();
    let mut compactions = 0;

    // Circuit breaker: skip compaction after repeated failures.
    if state.compaction_failures() >= MAX_CONSECUTIVE_COMPACTION_FAILURES {
        info!(
            iteration,
            failures = state.compaction_failures(),
            "compaction circuit breaker open — skipping compaction this iteration"
        );
        return Ok(CompactionPhaseResult::Continue { events, compactions });
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
            events.push(event);
            return Ok(CompactionPhaseResult::AbortTurn { events });
        }
        // Soft threshold: compact to stay within budget.
        if estimated_tokens >= budget.compact_at_tokens
            && let Some(compaction) = session.config().compaction.clone()
        {
            // Persist a snapshot before mutating messages, so recovery is possible.
            let _ = super::super::session::persistence::save_pre_compact_snapshot(
                session.session_id(),
                session.config(),
                context.messages(),
            )
            .await;
            let started = TurnEvent::CompactionStarted { reason: "token_budget".into() };
            session.emit_turn_event(&started);
            events.push(started);
            match compaction.compact(context.messages_mut(), provider).await {
                Ok(true) => {
                    compactions += 1;
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
                    let completed =
                        TurnEvent::CompactionCompleted { reason: "token_budget".into() };
                    session.emit_turn_event(&completed);
                    events.push(completed);
                    return Err(e);
                }
            }
            let completed = TurnEvent::CompactionCompleted { reason: "token_budget".into() };
            session.emit_turn_event(&completed);
            events.push(completed);
        }
    }

    Ok(CompactionPhaseResult::Continue { events, compactions })
}
