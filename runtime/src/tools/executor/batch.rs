//! Batch construction and concurrent execution for the tool executor.

use crate::config::AgentConfig;
use crate::model::message::ToolCall;
use crate::tools::api::{ToolContext, ToolRegistry};
use std::sync::Arc;

use super::types::{Batch, PreparedCall};
pub(crate) fn build_batches(
    calls: Vec<ToolCall>,
    tools: &ToolRegistry,
    config: &AgentConfig,
    session_id: &str,
    turn_id: u64,
    messages: Arc<Vec<crate::model::message::Message>>,
    read_file_state: crate::tools::api::FileReadState,
) -> Vec<Batch> {
    let mut batches = Vec::<Batch>::new();
    for (index, call) in calls.into_iter().enumerate() {
        let context = ToolContext {
            session_id: session_id.to_string(),
            turn_id,
            tool_call_id: Some(call.id.clone()),
            cwd: config.cwd.clone(),
            env: config.env.clone(),
            messages: Arc::clone(&messages),
            progress: None,
            read_file_state: read_file_state.clone(),
            timeout: config
                .tool_timeout_ms
                .filter(|&ms| ms > 0)
                .map(std::time::Duration::from_millis),
            max_file_read_bytes: config.max_file_read_bytes,
        };
        let concurrency_safe = tools
            .get(&call.name)
            .ok()
            .map(|tool| tool.is_concurrency_safe(&call.arguments))
            .unwrap_or(false);
        if let Some(batch) = batches.last_mut()
            && batch.concurrency_safe
            && concurrency_safe
        {
            batch.calls.push(PreparedCall { index, call, context });
        } else {
            batches.push(Batch {
                concurrency_safe,
                calls: vec![PreparedCall { index, call, context }],
            });
        }
    }
    batches
}
