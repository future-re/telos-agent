//! Session-level metrics — lightweight counters updated by the runtime.
//!
//! [`SessionMetrics`] is embedded in [`AgentSession`](crate::AgentSession) and
//! accumulates counters across all turns. Callers can snapshot it via
//! [`AgentSession::metrics`](crate::AgentSession::metrics) to feed their own
//! monitoring stack (Prometheus, CloudWatch, etc.).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Accumulated metrics for an agent session, updated internally by the runtime.
///
/// All counters are monotonic across all turns. Clone is cheap — the struct
/// wraps `Arc`s so snapshots share the same underlying counters.
#[derive(Debug, Clone)]
pub struct SessionMetrics {
    inner: Arc<MetricsInner>,
}

#[derive(Debug)]
struct MetricsInner {
    total_input_tokens: AtomicUsize,
    total_output_tokens: AtomicUsize,
    total_prompt_cache_hit_tokens: AtomicUsize,
    total_prompt_cache_miss_tokens: AtomicUsize,
    total_tool_calls: AtomicUsize,
    total_tool_errors: AtomicUsize,
    total_iterations: AtomicUsize,
    compaction_count: AtomicUsize,
    turn_count: AtomicUsize,
    retry_count: AtomicUsize,
}

impl SessionMetrics {
    /// Create a fresh metrics accumulator.
    pub(crate) fn new() -> Self {
        Self::with_values(0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
    }

    /// Restore metrics from persisted counter values.
    pub(crate) fn with_values(
        total_input_tokens: usize,
        total_output_tokens: usize,
        total_prompt_cache_hit_tokens: usize,
        total_prompt_cache_miss_tokens: usize,
        total_tool_calls: usize,
        total_tool_errors: usize,
        total_iterations: usize,
        compaction_count: usize,
        turn_count: usize,
        retry_count: usize,
    ) -> Self {
        Self {
            inner: Arc::new(MetricsInner {
                total_input_tokens: AtomicUsize::new(total_input_tokens),
                total_output_tokens: AtomicUsize::new(total_output_tokens),
                total_prompt_cache_hit_tokens: AtomicUsize::new(total_prompt_cache_hit_tokens),
                total_prompt_cache_miss_tokens: AtomicUsize::new(total_prompt_cache_miss_tokens),
                total_tool_calls: AtomicUsize::new(total_tool_calls),
                total_tool_errors: AtomicUsize::new(total_tool_errors),
                total_iterations: AtomicUsize::new(total_iterations),
                compaction_count: AtomicUsize::new(compaction_count),
                turn_count: AtomicUsize::new(turn_count),
                retry_count: AtomicUsize::new(retry_count),
            }),
        }
    }

    // -- Read methods (public API) --

    /// Total input tokens consumed across all turns.
    pub fn total_input_tokens(&self) -> usize {
        self.inner.total_input_tokens.load(Ordering::Relaxed)
    }

    /// Total output tokens produced across all turns.
    pub fn total_output_tokens(&self) -> usize {
        self.inner.total_output_tokens.load(Ordering::Relaxed)
    }

    /// Total prompt tokens served from cache across all turns.
    pub fn total_prompt_cache_hit_tokens(&self) -> usize {
        self.inner.total_prompt_cache_hit_tokens.load(Ordering::Relaxed)
    }

    /// Total prompt tokens not served from cache across all turns.
    pub fn total_prompt_cache_miss_tokens(&self) -> usize {
        self.inner.total_prompt_cache_miss_tokens.load(Ordering::Relaxed)
    }

    /// Total number of tool calls executed.
    pub fn total_tool_calls(&self) -> usize {
        self.inner.total_tool_calls.load(Ordering::Relaxed)
    }

    /// Total number of tool calls that resulted in errors.
    pub fn total_tool_errors(&self) -> usize {
        self.inner.total_tool_errors.load(Ordering::Relaxed)
    }

    /// Total number of model ⇄ tool iterations across all turns.
    pub fn total_iterations(&self) -> usize {
        self.inner.total_iterations.load(Ordering::Relaxed)
    }

    /// Number of times compaction was triggered.
    pub fn compaction_count(&self) -> usize {
        self.inner.compaction_count.load(Ordering::Relaxed)
    }

    /// Number of turns completed.
    pub fn turn_count(&self) -> usize {
        self.inner.turn_count.load(Ordering::Relaxed)
    }

    /// Number of provider retries across all turns.
    pub fn retry_count(&self) -> usize {
        self.inner.retry_count.load(Ordering::Relaxed)
    }

    // -- Update methods (used by the runtime) --

    pub(crate) fn add_input_tokens(&self, n: usize) {
        self.inner.total_input_tokens.fetch_add(n, Ordering::Relaxed);
    }

    pub(crate) fn add_output_tokens(&self, n: usize) {
        self.inner.total_output_tokens.fetch_add(n, Ordering::Relaxed);
    }

    pub(crate) fn add_prompt_cache_hit_tokens(&self, n: usize) {
        self.inner.total_prompt_cache_hit_tokens.fetch_add(n, Ordering::Relaxed);
    }

    pub(crate) fn add_prompt_cache_miss_tokens(&self, n: usize) {
        self.inner.total_prompt_cache_miss_tokens.fetch_add(n, Ordering::Relaxed);
    }

    pub(crate) fn add_tool_call(&self) {
        self.inner.total_tool_calls.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn add_tool_error(&self) {
        self.inner.total_tool_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn add_iteration(&self) {
        self.inner.total_iterations.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn add_compaction(&self) {
        self.inner.compaction_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn add_turn(&self) {
        self.inner.turn_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn add_retry(&self) {
        self.inner.retry_count.fetch_add(1, Ordering::Relaxed);
    }

    // -- Checkpoint / restore (used by the runtime to roll back failed turns) --

    /// Snapshot current counter values for later restore.
    pub(crate) fn checkpoint(&self) -> MetricsCheckpoint {
        MetricsCheckpoint {
            total_input_tokens: self.inner.total_input_tokens.load(Ordering::Relaxed),
            total_output_tokens: self.inner.total_output_tokens.load(Ordering::Relaxed),
            total_prompt_cache_hit_tokens: self
                .inner
                .total_prompt_cache_hit_tokens
                .load(Ordering::Relaxed),
            total_prompt_cache_miss_tokens: self
                .inner
                .total_prompt_cache_miss_tokens
                .load(Ordering::Relaxed),
            total_tool_calls: self.inner.total_tool_calls.load(Ordering::Relaxed),
            total_tool_errors: self.inner.total_tool_errors.load(Ordering::Relaxed),
            total_iterations: self.inner.total_iterations.load(Ordering::Relaxed),
            compaction_count: self.inner.compaction_count.load(Ordering::Relaxed),
            turn_count: self.inner.turn_count.load(Ordering::Relaxed),
            retry_count: self.inner.retry_count.load(Ordering::Relaxed),
        }
    }

    /// Restore counters to previously snapshotted values.
    pub(crate) fn restore(&self, cp: &MetricsCheckpoint) {
        self.inner.total_input_tokens.store(cp.total_input_tokens, Ordering::Relaxed);
        self.inner.total_output_tokens.store(cp.total_output_tokens, Ordering::Relaxed);
        self.inner
            .total_prompt_cache_hit_tokens
            .store(cp.total_prompt_cache_hit_tokens, Ordering::Relaxed);
        self.inner
            .total_prompt_cache_miss_tokens
            .store(cp.total_prompt_cache_miss_tokens, Ordering::Relaxed);
        self.inner.total_tool_calls.store(cp.total_tool_calls, Ordering::Relaxed);
        self.inner.total_tool_errors.store(cp.total_tool_errors, Ordering::Relaxed);
        self.inner.total_iterations.store(cp.total_iterations, Ordering::Relaxed);
        self.inner.compaction_count.store(cp.compaction_count, Ordering::Relaxed);
        self.inner.turn_count.store(cp.turn_count, Ordering::Relaxed);
        self.inner.retry_count.store(cp.retry_count, Ordering::Relaxed);
    }
}

/// Opaque snapshot of [`SessionMetrics`] counter values.
pub(crate) struct MetricsCheckpoint {
    total_input_tokens: usize,
    total_output_tokens: usize,
    total_prompt_cache_hit_tokens: usize,
    total_prompt_cache_miss_tokens: usize,
    total_tool_calls: usize,
    total_tool_errors: usize,
    total_iterations: usize,
    compaction_count: usize,
    turn_count: usize,
    retry_count: usize,
}
