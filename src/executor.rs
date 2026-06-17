//! Tool execution engine with batching and streaming support.
//!
//! Tools marked [`is_concurrency_safe`](crate::Tool::is_concurrency_safe) are grouped into
//! concurrent batches; others run sequentially. Batches preserve the original
//! call order in their results so the model always sees deterministic output.

use async_stream::stream;
use futures_core::stream::Stream;
use futures_util::FutureExt;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::{Value, json};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::message::{ToolCall, ToolResult};
use tracing::{Instrument, debug, error, info_span, warn};
use crate::permissions::RuleDecision;
use crate::tool::{PermissionDecision, ToolContext, ToolProgress, ToolRegistry};
use crate::tool::validate::validate_arguments_or_error;

/// Lifecycle event emitted by the executor for one tool invocation.
#[derive(Debug, Clone)]
pub enum ToolExecutionEvent {
    /// Emitted once when the tool starts (after permission, before `invoke`).
    ToolStarted { tool_call_id: String, name: String },
    /// Streaming progress update from inside the tool.
    ToolProgress {
        tool_call_id: Option<String>,
        name: String,
        message: String,
        data: Option<Value>,
    },
    /// Emitted once when the tool finishes (success or error).
    ToolCompleted {
        tool_call_id: String,
        name: String,
        is_error: bool,
    },
    /// A tool call has been suspended pending human approval.
    ApprovalRequested {
        tool_call_id: String,
        name: String,
        reason: String,
    },
    /// Human approval has been resolved for a suspended tool call.
    ApprovalResolved {
        tool_call_id: String,
        name: String,
        decision: String,
    },
}

/// Buffered output of [`execute_tool_calls`] — events in chronological order,
/// results in the original call order.
#[derive(Debug, Clone)]
pub struct ToolExecutionOutput {
    /// Every [`ToolExecutionEvent`] emitted during the batch, in fire order.
    pub events: Vec<ToolExecutionEvent>,
    /// One [`ToolResult`] per input call, restored to declaration order.
    pub results: Vec<ToolResult>,
}

/// A single tool call paired with the context the executor will hand to it.
#[derive(Debug, Clone)]
struct PreparedCall {
    /// Position in the original call list — used to restore deterministic order after concurrent execution.
    index: usize,
    call: ToolCall,
    context: ToolContext,
}

/// A contiguous run of calls that can either all run in parallel (when
/// `concurrency_safe`) or must run sequentially.
#[derive(Debug, Clone)]
struct Batch {
    concurrency_safe: bool,
    calls: Vec<PreparedCall>,
}

/// Non-streaming variant: run every tool call and return all events + results
/// in one shot. Used by callers that don't need progressive updates.
pub async fn execute_tool_calls(
    calls: Vec<ToolCall>,
    tools: &ToolRegistry,
    config: &AgentConfig,
    session_id: &str,
    turn_id: u64,
    messages: Vec<crate::message::Message>,
    read_file_state: crate::tool::FileReadState,
) -> ToolExecutionOutput {
    let mut output = ToolExecutionOutput {
        events: Vec::new(),
        results: Vec::new(),
    };

    // Partition the call list into contiguous batches of like-flavoured calls.
    // Switching from concurrency-safe to non-safe (or vice versa) starts a new batch.
    let mut batches = Vec::<Batch>::new();
    for (index, call) in calls.into_iter().enumerate() {
        let context = ToolContext {
            session_id: session_id.to_string(),
            turn_id,
            cwd: config.cwd.clone(),
            env: config.env.clone(),
            messages: messages.clone(),
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
        if concurrency_safe
            && batches
                .last()
                .map(|batch| batch.concurrency_safe)
                .unwrap_or(false)
        {
            batches.last_mut().unwrap().calls.push(PreparedCall {
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

/// Streaming variant: yield events and results as they happen.
///
/// Within a concurrency-safe batch, events from different calls are
/// interleaved (good for live UIs), but final [`ToolResult`]s are reordered
/// to match the original call order before being yielded.
pub fn execute_tool_calls_stream<'a>(
    calls: Vec<ToolCall>,
    tools: &'a ToolRegistry,
    config: &'a AgentConfig,
    session_id: &'a str,
    turn_id: u64,
    messages: Vec<crate::message::Message>,
    read_file_state: crate::tool::FileReadState,
) -> impl Stream<Item = ToolExecutionStreamItem> + 'a {
    stream! {
        // Batch identically to the non-streaming variant; see [`execute_tool_calls`] for the rationale.
        let mut batches = Vec::<Batch>::new();
        for (index, call) in calls.into_iter().enumerate() {
            let context = ToolContext {
                session_id: session_id.to_string(),
                turn_id,
                cwd: config.cwd.clone(),
                env: config.env.clone(),
                messages: messages.clone(),
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
            if concurrency_safe
                && batches
                    .last()
                    .map(|batch| batch.concurrency_safe)
                    .unwrap_or(false)
            {
                batches.last_mut().unwrap().calls.push(PreparedCall {
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

/// Items yielded by [`execute_tool_calls_stream`] — lifecycle events as they
/// happen, and final results once each call completes.
#[derive(Debug, Clone)]
pub enum ToolExecutionStreamItem {
    /// An [`ToolExecutionEvent`] — informational; may be emitted out of call order.
    Event(ToolExecutionEvent),
    /// A finished tool's [`ToolResult`] — emitted in the original call order at end of batch.
    Result(ToolResult),
}

/// Run a batch of concurrency-safe calls in parallel (used by the non-streaming path).
///
/// Maintains call ordering in the output so the model sees results in the
/// same order it requested them.
async fn run_concurrent_batch(
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
    output
        .results
        .extend(completed.into_iter().map(|(_, result)| result));
}

/// Run a single tool to completion, isolated in its own task so a panic
/// inside the tool cannot collapse the executor.
async fn run_one_tool(
    prepared: PreparedCall,
    tools: &ToolRegistry,
    config: &AgentConfig,
) -> (usize, Vec<ToolExecutionEvent>, ToolResult) {
    let index = prepared.index;
    let tool_call_id = prepared.call.id.clone();
    let name = prepared.call.name.clone();
    let tools = tools.clone();
    let config = config.clone();

    let handle = tokio::spawn(async move {
        AssertUnwindSafe(run_one_tool_inner(prepared, &tools, &config))
            .catch_unwind()
            .await
    });

    match handle.await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) | Err(_) => {
            warn!(tool = %name, tool_call_id = %tool_call_id, "tool invocation panicked");
            let events = vec![
                ToolExecutionEvent::ToolStarted {
                    tool_call_id: tool_call_id.clone(),
                    name: name.clone(),
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
                    content: json_error_payload("execution_panic", "tool invocation panicked".to_string()),
                    is_error: true,
                },
            )
        }
    }
}

/// Synchronous core of [`run_one_tool`] — runs the tool and collects its events.
async fn run_one_tool_inner(
    prepared: PreparedCall,
    tools: &ToolRegistry,
    config: &AgentConfig,
) -> (usize, Vec<ToolExecutionEvent>, ToolResult) {
    let mut events = vec![ToolExecutionEvent::ToolStarted {
        tool_call_id: prepared.call.id.clone(),
        name: prepared.call.name.clone(),
    }];

    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<ToolProgress>();
    let mut context = prepared.context;
    context.progress = Some(progress_tx);

    let (mut approval_events, result) = match tools.get(&prepared.call.name) {
        Ok(tool) => invoke_existing_tool(prepared.call.clone(), tool, context, config).await,
        Err(err) => (
            Vec::new(),
            ToolResult {
                tool_call_id: prepared.call.id.clone(),
                name: prepared.call.name.clone(),
                content: json_error_payload("tool_not_found", err.to_string()),
                is_error: true,
            },
        ),
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

/// Spawn a tokio task (tracked by `join_set`) that runs one tool call and
/// streams its events on `tx`.
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

/// Synchronous core of [`spawn_live_tool`] — runs the tool and forwards
/// events/results on `tx`.
async fn run_live_tool_inner(
    prepared: PreparedCall,
    tools: ToolRegistry,
    config: AgentConfig,
    tx: tokio::sync::mpsc::UnboundedSender<(usize, ToolExecutionStreamItem)>,
) {
    let index = prepared.index;
    let _ = tx.send((
        index,
        ToolExecutionStreamItem::Event(ToolExecutionEvent::ToolStarted {
            tool_call_id: prepared.call.id.clone(),
            name: prepared.call.name.clone(),
        }),
    ));

    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<ToolProgress>();
    let mut context = prepared.context;
    context.progress = Some(progress_tx);

    let call = prepared.call.clone();
    let tool = tools.get(&prepared.call.name);
    let result_task = async move {
        match tool {
            Ok(tool) => invoke_existing_tool(call, tool, context, &config).await,
            Err(err) => (
                Vec::new(),
                ToolResult {
                    tool_call_id: call.id.clone(),
                    name: call.name.clone(),
                    content: json_error_payload("tool_not_found", err.to_string()),
                    is_error: true,
                },
            ),
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

/// Run a tool through the full validate → permission → approval → invoke pipeline.
///
/// All failure modes (validation, permission, execution) are turned into
/// `is_error: true` [`ToolResult`]s rather than propagating — the model needs
/// a tool result for every tool call it made so it can react and recover.
///
/// Returns any lifecycle events (e.g. approval requests) alongside the result.
async fn invoke_existing_tool(
    mut call: ToolCall,
    tool: Arc<dyn crate::tool::Tool>,
    context: ToolContext,
    config: &AgentConfig,
) -> (Vec<ToolExecutionEvent>, ToolResult) {
    match tool.validate(&call.arguments, &context).await {
        Ok(()) => {
            // Run JSON Schema validation after the tool's custom validation so
            // both business rules and schema shape are enforced.
            if config.auto_validate_schema {
                let schema = tool.definition().input_schema;
                if let Err(err) =
                    validate_arguments_or_error(&call.name, &schema, &call.arguments)
                {
                    return (
                        Vec::new(),
                        ToolResult {
                            tool_call_id: call.id,
                            name: call.name,
                            content: json_error_payload("validation_error", err.to_string()),
                            is_error: true,
                        },
                    );
                }
            }

            // The global permission engine wins if it has a rule for this
            // call; otherwise we ask the tool itself.
            let canonical_name = tool.definition().name;
            let mut permission_names = vec![call.name.clone()];
            if canonical_name != call.name {
                permission_names.push(canonical_name.clone());
            }
            for alias in tool.aliases() {
                if !permission_names.iter().any(|n| n == alias) {
                    permission_names.push((*alias).to_string());
                }
            }
            let permission_names_ref: Vec<&str> =
                permission_names.iter().map(|s| s.as_str()).collect();
            let is_shell_tool = canonical_name == "Bash";
            let engine_decision = config.permission_engine.as_ref().and_then(|engine| {
                if is_shell_tool {
                    let command = call
                        .arguments
                        .get("command")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    engine.evaluate_shell_call(
                        &permission_names_ref,
                        command,
                        &call.arguments,
                        &context.cwd,
                    )
                } else {
                    engine.evaluate_call_any(&permission_names_ref, &call.arguments, &context.cwd)
                }
            });
            let permission = match engine_decision {
                Some(RuleDecision::Allow) => Ok(PermissionDecision::Allow),
                Some(RuleDecision::Deny) => Ok(PermissionDecision::Deny {
                    reason: "denied by permission rule".into(),
                }),
                Some(RuleDecision::Ask) => Ok(PermissionDecision::Ask {
                    reason: "approval required by permission rule".into(),
                }),
                None => tool.check_permission(&call.arguments, &context).await,
            };

            let mut events = Vec::new();
            let permission = match permission {
                Ok(PermissionDecision::Ask { reason }) => {
                    // If an approval handler is configured, suspend the call and
                    // ask for explicit human approval. Otherwise fall through to
                    // the legacy error-result behaviour.
                    if let Some(handler) = &config.approval_handler {
                        events.push(ToolExecutionEvent::ApprovalRequested {
                            tool_call_id: call.id.clone(),
                            name: call.name.clone(),
                            reason: reason.clone(),
                        });
                        let request = crate::approval::ApprovalRequest {
                            tool_name: canonical_name.clone(),
                            invocation_names: permission_names.clone(),
                            arguments: call.arguments.clone(),
                            cwd: context.cwd.clone(),
                            messages: context.messages.clone(),
                            reason: reason.clone(),
                        };
                        let decision = handler.ask(request).await;
                        events.push(ToolExecutionEvent::ApprovalResolved {
                            tool_call_id: call.id.clone(),
                            name: call.name.clone(),
                            decision: format!("{decision:?}"),
                        });
                        match decision {
                            crate::approval::ApprovalDecision::Allow => Ok(PermissionDecision::Allow),
                            crate::approval::ApprovalDecision::Deny { reason } => {
                                Ok(PermissionDecision::Deny { reason })
                            }
                            crate::approval::ApprovalDecision::Modify { arguments } => {
                                call.arguments = arguments;
                                Ok(PermissionDecision::Allow)
                            }
                        }
                    } else {
                        Ok(PermissionDecision::Ask { reason })
                    }
                }
                other => other,
            };

            match permission {
                Ok(PermissionDecision::Allow) => {
                    let invoke_span =
                        info_span!("tool_execution", tool = %call.name, tool_call_id = %call.id);
                    let tool_name = call.name.clone();
                    let invoke_result = {
                        let invoke_fut = tool.invoke(call.arguments.clone(), context);
                        // A timeout of 0ms is treated as "no timeout" to avoid
                        // immediately failing every tool call.
                        let timeout = config.tool_timeout_ms.filter(|&ms| ms > 0);
                        async move {
                            if let Some(ms) = timeout {
                                match tokio::time::timeout(
                                    std::time::Duration::from_millis(ms),
                                    invoke_fut,
                                )
                                .await
                                {
                                    Ok(result) => result,
                                    Err(_elapsed) => {
                                        warn!("tool timed out after {}ms", ms);
                                        Err(AgentError::ToolExecution {
                                            tool: tool_name,
                                            message: format!("timed out after {}ms", ms),
                                        })
                                    }
                                }
                            } else {
                                invoke_fut.await
                            }
                        }
                        .instrument(invoke_span)
                        .await
                    };
                    match invoke_result {
                        Ok(output) => {
                            debug!("tool succeeded");
                            (
                                events,
                                ToolResult {
                                    tool_call_id: call.id,
                                    name: call.name,
                                    content: output.content,
                                    is_error: false,
                                },
                            )
                        }
                        Err(err) => {
                            error!(error = %err, "tool failed");
                            (
                                events,
                                ToolResult {
                                    tool_call_id: call.id,
                                    name: call.name.clone(),
                                    content: json_error_payload("execution_error", err.to_string()),
                                    is_error: true,
                                },
                            )
                        }
                    }
                }
                Ok(PermissionDecision::Deny { reason }) => (
                    events,
                    ToolResult {
                        tool_call_id: call.id,
                        name: call.name.clone(),
                        content: json_error_payload("permission_denied", reason),
                        is_error: true,
                    },
                ),
                Ok(PermissionDecision::Ask { reason }) => (
                    events,
                    ToolResult {
                        tool_call_id: call.id,
                        name: call.name.clone(),
                        content: json_error_payload("permission_required", reason),
                        is_error: true,
                    },
                ),
                Err(err) => (
                    events,
                    ToolResult {
                        tool_call_id: call.id,
                        name: call.name.clone(),
                        content: json_error_payload("permission_error", err.to_string()),
                        is_error: true,
                    },
                ),
            }
        }
        Err(err) => (
            Vec::new(),
            ToolResult {
                tool_call_id: call.id,
                name: call.name.clone(),
                content: json_error_payload("validation_error", err.to_string()),
                is_error: true,
            },
        ),
    }
}

/// Build a structured `{ "error": { "kind", "message" } }` payload so the
/// model can pattern-match on `kind` instead of parsing free text.
fn json_error_payload(kind: &str, message: String) -> Value {
    json!({
        "error": {
            "kind": kind,
            "message": message,
        }
    })
}
