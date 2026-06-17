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
        Self {
            inner: Arc::new(MetricsInner {
                total_input_tokens: AtomicUsize::new(0),
                total_output_tokens: AtomicUsize::new(0),
                total_tool_calls: AtomicUsize::new(0),
                total_tool_errors: AtomicUsize::new(0),
                total_iterations: AtomicUsize::new(0),
                compaction_count: AtomicUsize::new(0),
                turn_count: AtomicUsize::new(0),
                retry_count: AtomicUsize::new(0),
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
}
