//! Synchronous (non-streaming) tool execution path.

use crate::config::AgentConfig;
use crate::diagnostics::ToolFailureKind;
use crate::message::{ToolCall, ToolResult};
use crate::tool::{ToolContext, ToolProgress, ToolRegistry};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tracing::warn;

use super::batch::run_concurrent_batch;
use super::invoke::{invoke_existing_tool, json_error_payload, record_tool_failure};
use super::types::{Batch, PreparedCall, ToolExecutionEvent, ToolExecutionOutput};

pub async fn execute_tool_calls(
    calls: Vec<ToolCall>,
    tools: &ToolRegistry,
    config: &AgentConfig,
    session_id: &str,
    turn_id: u64,
    messages: Arc<Vec<crate::message::Message>>,
    read_file_state: crate::tool::FileReadState,
) -> ToolExecutionOutput {
    let mut output = ToolExecutionOutput { events: Vec::new(), results: Vec::new() };

    // Partition the call list into contiguous batches of like-flavoured calls.
    // Switching from concurrency-safe to non-safe (or vice versa) starts a new batch.
    let mut batches = Vec::<Batch>::new();
    for (index, call) in calls.into_iter().enumerate() {
        let context = ToolContext {
            session_id: session_id.to_string(),
            turn_id,
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

    for batch in batches {
        if batch.concurrency_safe && config.tool_concurrency_limit > 1 {
            run_concurrent_batch(batch, tools, config, &mut output).await;
        } else {
            // Serial fallback: run one at a time, in declaration order.
            for prepared in batch.calls {
                let (_, events, result) = run_one_tool(prepared, tools, config).await;
                output.events.extend(events);
                output.results.push(result);
            }
        }
    }

    output
}

pub(crate) async fn run_one_tool(
    prepared: PreparedCall,
    tools: &ToolRegistry,
    config: &AgentConfig,
) -> (usize, Vec<ToolExecutionEvent>, ToolResult) {
    let index = prepared.index;
    let tool_call_id = prepared.call.id.clone();
    let name = prepared.call.name.clone();
    let panic_call = prepared.call.clone();
    let panic_context = prepared.context.clone();
    let tools = tools.clone();
    let config = config.clone();
    let task_config = config.clone();

    let handle = tokio::spawn(async move {
        AssertUnwindSafe(run_one_tool_inner(prepared, &tools, &task_config)).catch_unwind().await
    });

    match handle.await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) | Err(_) => {
            warn!(tool = %name, tool_call_id = %tool_call_id, "tool invocation panicked");
            record_tool_failure(
                &config,
                &panic_context,
                &panic_call,
                ToolFailureKind::ExecutionPanic,
                "tool invocation panicked",
            )
            .await;
            let events = vec![
                ToolExecutionEvent::ToolStarted {
                    tool_call_id: tool_call_id.clone(),
                    name: name.clone(),
                    detail: String::new(),
                },
                ToolExecutionEvent::ToolCompleted {
                    tool_call_id: tool_call_id.clone(),
                    name: name.clone(),
                    is_error: true,
                },
            ];
            (
                index,
                events,
                ToolResult {
                    tool_call_id,
                    name,
                    content: json_error_payload(
                        "execution_panic",
                        "tool invocation panicked".to_string(),
                    ),
                    is_error: true,
                },
            )
        }
    }
}

async fn run_one_tool_inner(
    prepared: PreparedCall,
    tools: &ToolRegistry,
    config: &AgentConfig,
) -> (usize, Vec<ToolExecutionEvent>, ToolResult) {
    let detail = super::stream::tool_detail(&prepared.call.name, &prepared.call.arguments);
    let mut events = vec![ToolExecutionEvent::ToolStarted {
        tool_call_id: prepared.call.id.clone(),
        name: prepared.call.name.clone(),
        detail,
    }];

    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<ToolProgress>();
    let mut context = prepared.context;
    context.progress = Some(progress_tx);

    let (mut approval_events, result) = match tools.get(&prepared.call.name) {
        Ok(tool) => invoke_existing_tool(prepared.call.clone(), tool, context, config, tools).await,
        Err(err) => {
            record_tool_failure(
                config,
                &context,
                &prepared.call,
                ToolFailureKind::ToolNotFound,
                &err.to_string(),
            )
            .await;
            (
                Vec::new(),
                ToolResult {
                    tool_call_id: prepared.call.id.clone(),
                    name: prepared.call.name.clone(),
                    content: json_error_payload("tool_not_found", err.to_string()),
                    is_error: true,
                },
            )
        }
    };
    events.append(&mut approval_events);

    while let Ok(progress) = progress_rx.try_recv() {
        events.push(ToolExecutionEvent::ToolProgress {
            tool_call_id: progress.tool_call_id,
            name: prepared.call.name.clone(),
            message: progress.message,
            data: progress.data,
        });
    }

    events.push(ToolExecutionEvent::ToolCompleted {
        tool_call_id: result.tool_call_id.clone(),
        name: result.name.clone(),
        is_error: result.is_error,
    });

    (prepared.index, events, result)
}
