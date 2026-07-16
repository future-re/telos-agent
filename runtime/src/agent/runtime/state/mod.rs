use std::collections::HashMap;
use std::sync::Arc;

use crate::metrics::SessionMetrics;
use crate::tools::api::FileReadState;

pub(crate) struct RuntimeState {
    pub(crate) metrics: SessionMetrics,
    pub(crate) read_file_state: FileReadState,
    pub(crate) consecutive_compaction_failures: usize,
}

impl RuntimeState {
    pub fn new() -> Self {
        Self {
            metrics: SessionMetrics::new(),
            read_file_state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            consecutive_compaction_failures: 0,
        }
    }

    pub fn with_values(
        metrics: SessionMetrics,
        read_file_state: FileReadState,
        consecutive_compaction_failures: usize,
    ) -> Self {
        Self { metrics, read_file_state, consecutive_compaction_failures }
    }
}

impl RuntimeState {
    pub(crate) fn metrics(&self) -> &SessionMetrics {
        &self.metrics
    }

    pub(crate) fn metrics_mut(&mut self) -> &mut SessionMetrics {
        &mut self.metrics
    }

    pub(crate) fn read_file_state(&self) -> &FileReadState {
        &self.read_file_state
    }

    pub(crate) fn compaction_failures(&self) -> usize {
        self.consecutive_compaction_failures
    }

    pub(crate) fn set_compaction_failures(&mut self, val: usize) {
        self.consecutive_compaction_failures = val;
    }
}
