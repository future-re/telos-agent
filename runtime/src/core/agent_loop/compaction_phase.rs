use tracing::{info, warn};

use crate::compaction::estimate_message_tokens;
use crate::context::ContextOps;
use crate::error::AgentError;
use crate::provider::ModelProvider;
use crate::session::SessionOps;
use crate::state::StateOps;
use crate::turn::TurnEvent;

use super::MAX_CONSECUTIVE_COMPACTION_FAILURES;

/// Outcome of the compaction phase.
pub(super) enum CompactionPhaseResult {
    /// Proceed with the current turn, possibly with compaction events recorded.
    Continue { events: Vec<TurnEvent>, compactions: usize },
    /// The turn must be aborted (e.g., token budget is already exceeded).
    AbortTurn { events: Vec<TurnEvent> },
}

/// Checks token budget and triggers compaction if needed.
///
/// Two guard conditions prevent compaction from running:
/// - **Circuit breaker** — if consecutive compaction failures exceed
///   `MAX_CONSECUTIVE_COMPACTION_FAILURES`, compaction is skipped entirely.
/// - **Token budget exceeded** — if estimated tokens already surpass the
///   hard `max_tokens` cap, the turn is aborted immediately.
///
/// When neither guard fires and the estimated token count is at or above
/// `compact_at_tokens`, a pre-compact snapshot is persisted, then
/// compaction runs. On success a system reminder is injected so the model
/// is aware of the summarisation; on failure the error is propagated and
/// the failure counter is bumped.
///
/// # Errors
/// Returns `AgentError` if the compaction strategy itself fails. A token
/// budget overrun is **not** an error — the turn is aborted via
/// `CompactionPhaseResult::AbortTurn`.
pub(super) async fn run_compaction_phase<S, C, St, P>(
    session: &mut S,
    context: &mut C,
    state: &mut St,
    provider: &P,
    iteration: usize,
) -> Result<CompactionPhaseResult, AgentError>
where
    S: SessionOps,
    C: ContextOps,
    St: StateOps,
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
            events.push(TurnEvent::TokenBudgetExceeded {
                used_tokens: estimated_tokens,
                max_tokens: budget.max_tokens,
            });
            return Ok(CompactionPhaseResult::AbortTurn { events });
        }
        // Soft threshold: compact to stay within budget.
        if estimated_tokens >= budget.compact_at_tokens
            && let Some(compaction) = session.config().compaction.clone()
        {
            // Persist a snapshot before mutating messages, so recovery is possible.
            let _ = crate::session::persistence::save_pre_compact_snapshot(
                session.session_id(),
                session.config(),
                context.messages(),
            )
            .await;
            events.push(TurnEvent::CompactionStarted { reason: "token_budget".into() });
            match compaction.compact(context.messages_mut(), provider).await {
                Ok(true) => {
                    compactions += 1;
                    state.set_compaction_failures(0);
                    context.push_system_reminder(crate::message::SystemReminder::Compaction {
                        reason: "token_budget".into(),
                    });
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
                    events
                        .push(TurnEvent::CompactionCompleted { reason: "token_budget".into() });
                    return Err(e);
                }
            }
            events.push(TurnEvent::CompactionCompleted { reason: "token_budget".into() });
        }
    }

    Ok(CompactionPhaseResult::Continue { events, compactions })
}
