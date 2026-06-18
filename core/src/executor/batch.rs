//! Concurrent batch execution for the tool executor.

use crate::config::AgentConfig;
use crate::tool::ToolRegistry;
use futures_util::stream::{FuturesUnordered, StreamExt};

use super::sync::run_one_tool;
use super::types::{Batch, ToolExecutionOutput};

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
