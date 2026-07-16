//! Streaming tool execution path.

use crate::config::AgentConfig;
use crate::diagnostics::ToolFailureKind;
use crate::model::message::{ToolCall, ToolResult};
use crate::tools::api::{ToolProgress, ToolRegistry};
use async_stream::stream;
use futures_core::stream::Stream;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tracing::warn;

use super::batch::build_batches;
use super::invoke::{invoke_tool, json_error_payload, record_tool_failure, tool_detail};
use super::types::{PreparedCall, ToolExecutionEvent, ToolExecutionStreamItem};

enum WorkerMessage {
    Event(ToolExecutionEvent),
    Done { index: usize, result: ToolResult },
}

pub fn execute_tool_calls_stream<'a>(
    calls: Vec<ToolCall>,
    tools: &'a ToolRegistry,
    config: &'a AgentConfig,
    session_id: &'a str,
    turn_id: u64,
    messages: Arc<Vec<crate::model::message::Message>>,
    read_file_state: crate::tools::api::FileReadState,
) -> impl Stream<Item = ToolExecutionStreamItem> + 'a {
    let batches =
        build_batches(calls, tools, config, session_id, turn_id, messages, read_file_state);

    stream! {
        for batch in batches {
            let items = if batch.concurrency_safe && config.tool_concurrency_limit > 1 {
                execute_concurrent_batch(batch.calls, tools.clone(), config.clone()).await
            } else {
                execute_sequential_batch(batch.calls, tools.clone(), config.clone()).await
            };
            for item in items {
                yield item;
            }
        }
    }
}

async fn execute_concurrent_batch(
    calls: Vec<PreparedCall>,
    tools: ToolRegistry,
    config: AgentConfig,
) -> Vec<ToolExecutionStreamItem> {
    let limit = config.tool_concurrency_limit;
    let mut queued = calls.into_iter().peekable();
    let mut active = 0usize;
    let mut completed = Vec::new();
    let mut items = Vec::new();
    let (send, mut recv) = tokio::sync::mpsc::unbounded_channel::<WorkerMessage>();
    let worker_tx = send.clone();
    let mut join_set = tokio::task::JoinSet::new();

    while active < limit {
        let Some(prepared) = queued.next() else {
            break;
        };
        active += 1;
        spawn_tool_event_worker(
            &mut join_set,
            prepared,
            tools.clone(),
            config.clone(),
            worker_tx.clone(),
        );
    }
    // Done sending, so drop the sender to close the channel when all workers are done.
    drop(send);

    loop {
        match recv.recv().await {
            Some(WorkerMessage::Event(event)) => {
                items.push(ToolExecutionStreamItem::Event(event));
            }
            Some(WorkerMessage::Done { index, result }) => {
                active -= 1;
                completed.push((index, result));

                if let Some(prepared) = queued.next() {
                    active += 1;
                    spawn_tool_event_worker(
                        &mut join_set,
                        prepared,
                        tools.clone(),
                        config.clone(),
                        worker_tx.clone(),
                    );
                }

                if active == 0 {
                    break;
                }
            }
            None => break,
        }
    }

    // task done, so drop the join set to avoid holding onto any tasks.
    drop(join_set);

    completed.sort_by_key(|(index, _)| *index);
    for (_, result) in completed {
        items.push(ToolExecutionStreamItem::Result(result));
    }

    items
}

async fn execute_sequential_batch(
    calls: Vec<PreparedCall>,
    tools: ToolRegistry,
    config: AgentConfig,
) -> Vec<ToolExecutionStreamItem> {
    let mut items = Vec::new();
    for prepared in calls {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WorkerMessage>();
        let mut join_set = tokio::task::JoinSet::new();
        spawn_tool_event_worker(&mut join_set, prepared, tools.clone(), config.clone(), tx);
        while let Some(msg) = rx.recv().await {
            match msg {
                WorkerMessage::Event(event) => items.push(ToolExecutionStreamItem::Event(event)),
                WorkerMessage::Done { index: _, result } => {
                    items.push(ToolExecutionStreamItem::Result(result))
                }
            }
        }
    }
    items
}

fn spawn_tool_event_worker(
    join_set: &mut tokio::task::JoinSet<()>,
    prepared: PreparedCall,
    tools: ToolRegistry,
    config: AgentConfig,
    tx: tokio::sync::mpsc::UnboundedSender<WorkerMessage>,
) {
    let index = prepared.index;
    let tool_call_id = prepared.call.id.clone();
    let name = prepared.call.name.clone();
    let panic_call = prepared.call.clone();
    let panic_context = prepared.context.clone();
    let panic_config = config.clone();

    join_set.spawn(async move {
        let result = AssertUnwindSafe(run_tool_with_event_forwarding(prepared, tools, config, tx.clone()))
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
            let _ = tx.send(WorkerMessage::Done {
                index,
                result: ToolResult {
                    tool_call_id,
                    name,
                    content: json_error_payload("execution_panic", message),
                    is_error: true,
                },
            });
        }
    });
}

async fn run_tool_with_event_forwarding(
    prepared: PreparedCall,
    tools: ToolRegistry,
    config: AgentConfig,
    tx: tokio::sync::mpsc::UnboundedSender<WorkerMessage>,
) {
    let index = prepared.index;
    let detail = tool_detail(&tools, &prepared.call.name, &prepared.call.arguments);
    let _ = tx.send(WorkerMessage::Event(ToolExecutionEvent::ToolStarted {
        tool_call_id: prepared.call.id.clone(),
        name: prepared.call.name.clone(),
        detail,
    }));

    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<ToolProgress>();
    let mut context = prepared.context;
    context.progress = Some(progress_tx);

    let call = prepared.call.clone();
    let tool = tools.get(&prepared.call.name);
    let context_for_not_found = context.clone();
    let result_task = async move {
        match tool {
            Ok(tool) => invoke_tool(call, tool, context, &config, &tools).await,
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
            maybe_progress = progress_rx.recv() => {
                if let Some(progress) = maybe_progress {
                    let _ = tx.send(WorkerMessage::Event(ToolExecutionEvent::ToolProgress {
                        tool_call_id: progress.tool_call_id,
                        name: prepared.call.name.clone(),
                        message: progress.message,
                        data: progress.data,
                    }));
                }
            }
            result = &mut result_task => {
                while let Ok(progress) = progress_rx.try_recv() {
                    let _ = tx.send(WorkerMessage::Event(ToolExecutionEvent::ToolProgress {
                        tool_call_id: progress.tool_call_id,
                        name: prepared.call.name.clone(),
                        message: progress.message,
                        data: progress.data,
                    }));
                }
                break result;
            }
        }
    };

    for event in approval_events {
        let _ = tx.send(WorkerMessage::Event(event));
    }

    let _ = tx.send(WorkerMessage::Done { index, result });
}
