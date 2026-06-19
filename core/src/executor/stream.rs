//! Streaming tool execution path.

use crate::config::AgentConfig;
use crate::diagnostics::ToolFailureKind;
use crate::message::{ToolCall, ToolResult};
use crate::tool::{ToolContext, ToolProgress, ToolRegistry};
use async_stream::stream;
use futures_core::stream::Stream;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tracing::warn;

use super::invoke::{invoke_existing_tool, json_error_payload, record_tool_failure};
use super::types::{Batch, PreparedCall, ToolExecutionEvent, ToolExecutionStreamItem};

pub fn execute_tool_calls_stream<'a>(
    calls: Vec<ToolCall>,
    tools: &'a ToolRegistry,
    config: &'a AgentConfig,
    session_id: &'a str,
    turn_id: u64,
    messages: Arc<Vec<crate::message::Message>>,
    read_file_state: crate::tool::FileReadState,
) -> impl Stream<Item = ToolExecutionStreamItem> + 'a {
    stream! {
        // Batch identically to the non-streaming variant; see [`execute_tool_calls`] for the rationale.
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
                timeout: config.tool_timeout_ms.filter(|&ms| ms > 0).map(std::time::Duration::from_millis),
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
                batch.calls.push(PreparedCall {
                    index,
                    call,
                    context,
                });
            } else {
                batches.push(Batch {
                    concurrency_safe,
                    calls: vec![PreparedCall {
                        index,
                        call,
                        context,
                    }],
                });
            }
        }

        for batch in batches {
            if batch.concurrency_safe && config.tool_concurrency_limit > 1 {
                // Fan out via tokio tasks tracked in a JoinSet. Each task
                // forwards events on a shared mpsc channel; we cap in-flight
                // tasks at `tool_concurrency_limit` and spawn more as slots
                // free up. JoinSet ensures tasks are cancelled if the consumer
                // drops the stream, and each task catches panics so a single
                // misbehaving tool cannot deadlock the batch.
                let mut queued = batch.calls.into_iter().peekable();
                let mut running = 0usize;
                let mut completed = Vec::new();
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(usize, ToolExecutionStreamItem)>();
                let tx_main = tx.clone();
                let mut join_set = tokio::task::JoinSet::new();

                while running < config.tool_concurrency_limit {
                    let Some(prepared) = queued.next() else {
                        break;
                    };
                    running += 1;
                    spawn_live_tool(&mut join_set, prepared, tools.clone(), config.clone(), tx_main.clone());
                }
                // Drop the local sender so `rx.recv()` returns None when every task exits.
                drop(tx);

                loop {
                    tokio::select! {
                        maybe_item = rx.recv() => {
                            match maybe_item {
                                Some((index, item)) => match item {
                                    ToolExecutionStreamItem::Event(event) => {
                                        // Forward events live — order is informational, not load-bearing.
                                        yield ToolExecutionStreamItem::Event(event);
                                    }
                                    ToolExecutionStreamItem::Result(result) => {
                                        running -= 1;
                                        // Hold results back until the batch finishes so we can sort by `index`.
                                        completed.push((index, result));
                                        if let Some(prepared) = queued.next() {
                                            running += 1;
                                            spawn_live_tool(&mut join_set, prepared, tools.clone(), config.clone(), tx_main.clone());
                                        }
                                        if running == 0 {
                                            break;
                                        }
                                    }
                                }
                                None => break,
                            }
                        }
                        maybe_done = join_set.join_next(), if !join_set.is_empty() => {
                            // Tasks catch their own panics, so this is mostly
                            // for bookkeeping and to avoid leaving completed
                            // tasks in the set.
                            if maybe_done.is_none() && running == 0 {
                                break;
                            }
                        }
                    }
                }
                // `join_set` is dropped here, aborting any still-running tasks
                // if the consumer dropped the stream early.
                drop(join_set);

                // Restore deterministic order before yielding results downstream.
                completed.sort_by_key(|(index, _)| *index);
                for (_, result) in completed {
                    yield ToolExecutionStreamItem::Result(result);
                }
            } else {
                // Serial path: one task per call; drain to completion before starting the next.
                for prepared in batch.calls {
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(usize, ToolExecutionStreamItem)>();
                    let mut join_set = tokio::task::JoinSet::new();
                    spawn_live_tool(&mut join_set, prepared, tools.clone(), config.clone(), tx);
                    while let Some((_, item)) = rx.recv().await {
                        yield item;
                    }
                    // join_set is dropped here; aborts the task if the consumer
                    // dropped the stream before it finished.
                }
            }
        }
    }
}

fn spawn_live_tool(
    join_set: &mut tokio::task::JoinSet<()>,
    prepared: PreparedCall,
    tools: ToolRegistry,
    config: AgentConfig,
    tx: tokio::sync::mpsc::UnboundedSender<(usize, ToolExecutionStreamItem)>,
) {
    let index = prepared.index;
    let tool_call_id = prepared.call.id.clone();
    let name = prepared.call.name.clone();
    let panic_call = prepared.call.clone();
    let panic_context = prepared.context.clone();
    let panic_config = config.clone();

    join_set.spawn(async move {
        let result = AssertUnwindSafe(run_live_tool_inner(prepared, tools, config, tx.clone()))
            .catch_unwind()
            .await;

        if let Err(ref err) = result {
            let message = if let Some(s) = err.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = err.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "tool invocation panicked".to_string()
            };
            warn!(tool = %name, tool_call_id = %tool_call_id, "tool invocation panicked: {message}");
            record_tool_failure(
                &panic_config,
                &panic_context,
                &panic_call,
                ToolFailureKind::ExecutionPanic,
                &message,
            )
            .await;
            let _ = tx.send((
                index,
                ToolExecutionStreamItem::Event(ToolExecutionEvent::ToolCompleted {
                    tool_call_id: tool_call_id.clone(),
                    name: name.clone(),
                    is_error: true,
                }),
            ));
            let _ = tx.send((
                index,
                ToolExecutionStreamItem::Result(ToolResult {
                    tool_call_id,
                    name,
                    content: json_error_payload("execution_panic", message),
                    is_error: true,
                }),
            ));
        }
    });
}

async fn run_live_tool_inner(
    prepared: PreparedCall,
    tools: ToolRegistry,
    config: AgentConfig,
    tx: tokio::sync::mpsc::UnboundedSender<(usize, ToolExecutionStreamItem)>,
) {
    let index = prepared.index;
    let detail = tool_detail(&prepared.call.name, &prepared.call.arguments);
    let _ = tx.send((
        index,
        ToolExecutionStreamItem::Event(ToolExecutionEvent::ToolStarted {
            tool_call_id: prepared.call.id.clone(),
            name: prepared.call.name.clone(),
            detail,
        }),
    ));

    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<ToolProgress>();
    let mut context = prepared.context;
    context.progress = Some(progress_tx);

    let call = prepared.call.clone();
    let tool = tools.get(&prepared.call.name);
    let context_for_not_found = context.clone();
    let result_task = async move {
        match tool {
            Ok(tool) => invoke_existing_tool(call, tool, context, &config, &tools).await,
            Err(err) => {
                record_tool_failure(
                    &config,
                    &context_for_not_found,
                    &call,
                    ToolFailureKind::ToolNotFound,
                    &err.to_string(),
                )
                .await;
                (
                    Vec::new(),
                    ToolResult {
                        tool_call_id: call.id.clone(),
                        name: call.name.clone(),
                        content: json_error_payload("tool_not_found", err.to_string()),
                        is_error: true,
                    },
                )
            }
        }
    };
    tokio::pin!(result_task);

    let (approval_events, result) = loop {
        tokio::select! {
            // Race: progress event vs. tool completion.
            maybe_progress = progress_rx.recv() => {
                if let Some(progress) = maybe_progress {
                    let _ = tx.send((
                        index,
                        ToolExecutionStreamItem::Event(ToolExecutionEvent::ToolProgress {
                            tool_call_id: progress.tool_call_id,
                            name: prepared.call.name.clone(),
                            message: progress.message,
                            data: progress.data,
                        }),
                    ));
                }
            }
            result = &mut result_task => {
                // Drain any progress events queued in the same tick as completion.
                while let Ok(progress) = progress_rx.try_recv() {
                    let _ = tx.send((
                        index,
                        ToolExecutionStreamItem::Event(ToolExecutionEvent::ToolProgress {
                            tool_call_id: progress.tool_call_id,
                            name: prepared.call.name.clone(),
                            message: progress.message,
                            data: progress.data,
                        }),
                    ));
                }
                break result;
            }
        }
    };

    for event in approval_events {
        let _ = tx.send((index, ToolExecutionStreamItem::Event(event)));
    }

    let _ = tx.send((
        index,
        ToolExecutionStreamItem::Event(ToolExecutionEvent::ToolCompleted {
            tool_call_id: result.tool_call_id.clone(),
            name: result.name.clone(),
            is_error: result.is_error,
        }),
    ));
    let _ = tx.send((index, ToolExecutionStreamItem::Result(result)));
}

/// Extract a human-readable detail from a tool's arguments.
pub(crate) fn tool_detail(name: &str, args: &serde_json::Value) -> String {
    let name_lower = name.to_lowercase();
    match name_lower.as_str() {
        "bash" | "shell" => {
            args.get("command").and_then(|v| v.as_str()).map(truncate_cmd).unwrap_or_default()
        }
        "read" | "write" | "edit" => args
            .get("file_path")
            .or_else(|| args.get("path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "grep" | "glob" => {
            args.get("pattern").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default()
        }
        "websearch" => {
            args.get("query").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default()
        }
        "webfetch" => args
            .get("url")
            .or_else(|| args.get("urls"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "task" | "agent" => args
            .get("description")
            .or_else(|| args.get("prompt"))
            .and_then(|v| v.as_str())
            .map(truncate_cmd)
            .unwrap_or_default(),
        _ => args
            .get("command")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("path"))
            .or_else(|| args.get("pattern"))
            .or_else(|| args.get("query"))
            .or_else(|| args.get("url"))
            .or_else(|| args.get("description"))
            .and_then(|v| v.as_str())
            .map(truncate_cmd)
            .unwrap_or_default(),
    }
}

fn truncate_cmd(cmd: &str) -> String {
    let first_line = cmd.lines().next().unwrap_or(cmd);
    if first_line.len() > 120 {
        format!("{}…", &first_line[..117])
    } else {
        first_line.to_string()
    }
}
