//! Batch construction and concurrent execution for the tool executor.

use crate::config::AgentConfig;
use crate::message::ToolCall;
use crate::tool::{ToolContext, ToolRegistry};
use futures_util::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;

use super::sync::run_one_tool;
use super::types::{Batch, PreparedCall, ToolExecutionOutput};

pub(crate) fn build_batches(
    calls: Vec<ToolCall>,
    tools: &ToolRegistry,
    config: &AgentConfig,
    session_id: &str,
    turn_id: u64,
    messages: Arc<Vec<crate::message::Message>>,
    read_file_state: crate::tool::FileReadState,
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

pub(crate) async fn run_concurrent_batch(
    batch: Batch,
    tools: &ToolRegistry,
    config: &AgentConfig,
    output: &mut ToolExecutionOutput,
) {
    let mut pending = FuturesUnordered::new();
    let mut queued = batch.calls.into_iter();

    for _ in 0..config.tool_concurrency_limit {
        if let Some(prepared) = queued.next() {
            pending.push(run_one_tool(prepared, tools, config));
        }
    }

    let mut completed = Vec::new();
    while let Some((index, events, result)) = pending.next().await {
        output.events.extend(events);
        completed.push((index, result));
        if let Some(prepared) = queued.next() {
            pending.push(run_one_tool(prepared, tools, config));
        }
    }
    completed.sort_by_key(|(index, _)| *index);
    output.results.extend(completed.into_iter().map(|(_, result)| result));
}
