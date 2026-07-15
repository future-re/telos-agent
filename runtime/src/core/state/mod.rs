use std::collections::HashMap;
use std::sync::Arc;

use crate::metrics::SessionMetrics;
use crate::tool::FileReadState;

pub trait StateOps {
    fn metrics(&self) -> &SessionMetrics;
    fn metrics_mut(&mut self) -> &mut SessionMetrics;
    fn read_file_state(&self) -> &FileReadState;
    fn read_file_state_owned(&self) -> FileReadState;
    fn set_read_file_state(&mut self, state: FileReadState);
    fn compaction_failures(&self) -> usize;
    fn set_compaction_failures(&mut self, val: usize);
}

pub struct RuntimeState {
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

impl StateOps for RuntimeState {
    fn metrics(&self) -> &SessionMetrics {
        &self.metrics
    }

    fn metrics_mut(&mut self) -> &mut SessionMetrics {
        &mut self.metrics
    }

    fn read_file_state(&self) -> &FileReadState {
        &self.read_file_state
    }

    fn read_file_state_owned(&self) -> FileReadState {
        self.read_file_state.clone()
    }

    fn set_read_file_state(&mut self, state: FileReadState) {
        self.read_file_state = state;
    }

    fn compaction_failures(&self) -> usize {
        self.consecutive_compaction_failures
    }

    fn set_compaction_failures(&mut self, val: usize) {
        self.consecutive_compaction_failures = val;
    }
}
