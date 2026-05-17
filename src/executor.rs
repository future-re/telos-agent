//! Tool execution engine with batching and streaming support.
//!
//! Tools marked [`is_concurrency_safe`](crate::Tool::is_concurrency_safe) are grouped into
//! concurrent batches; others run sequentially. Batches preserve the original
//! call order in their results so the model always sees deterministic output.

use async_stream::stream;
use futures_core::stream::Stream;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::config::AgentConfig;
use crate::message::{ToolCall, ToolResult};
use crate::permissions::RuleDecision;
use crate::tool::{PermissionDecision, ToolContext, ToolProgress, ToolRegistry};

/// Lifecycle event emitted by the executor for one tool invocation.
#[derive(Debug, Clone)]
pub enum ToolExecutionEvent {
    /// Emitted once when the tool starts (after permission, before `invoke`).
    ToolStarted {
        tool_call_id: String,
        name: String,
    },
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
                // Fan out via tokio tasks. Each task forwards events on a
                // shared mpsc channel; we cap in-flight tasks at
                // `tool_concurrency_limit` and spawn more as slots free up.
                let mut queued = batch.calls.into_iter().peekable();
                let mut running = 0usize;
                let mut completed = Vec::new();
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(usize, ToolExecutionStreamItem)>();
                let tx_main = tx.clone();

                while running < config.tool_concurrency_limit {
                    let Some(prepared) = queued.next() else {
                        break;
                    };
                    running += 1;
                    spawn_live_tool(prepared, tools.clone(), config.clone(), tx.clone());
                }
                // Drop the local sender so `rx.recv()` returns None when every task exits.
                drop(tx);

                while running > 0 {
                    if let Some((index, item)) = rx.recv().await {
                        match item {
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
                                    spawn_live_tool(prepared, tools.clone(), config.clone(), tx_main.clone());
                                }
                            }
                        }
                    }
                }
                // Restore deterministic order before yielding results downstream.
                completed.sort_by_key(|(index, _)| *index);
                for (_, result) in completed {
                    yield ToolExecutionStreamItem::Result(result);
                }
            } else {
                // Serial path: one mpsc per call; drain to completion before starting the next.
                for prepared in batch.calls {
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(usize, ToolExecutionStreamItem)>();
                    spawn_live_tool(prepared, tools.clone(), config.clone(), tx);
                    while let Some((_, item)) = rx.recv().await {
                        yield item;
                    }
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

/// Run a single tool to completion synchronously and collect its events.
///
/// Progress events arrive on an mpsc channel — we drain it after the tool
/// finishes (best-effort) and emit them in order. Used only by the
/// non-streaming path; the streaming path uses [`spawn_live_tool`] for
/// real-time progress forwarding.
async fn run_one_tool(
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

    let result = match tools.get(&prepared.call.name) {
        Ok(tool) => invoke_existing_tool(prepared.call.clone(), tool, context, config).await,
        Err(err) => ToolResult {
            tool_call_id: prepared.call.id.clone(),
            name: prepared.call.name.clone(),
            content: json_error_payload("tool_not_found", err.to_string()),
            is_error: true,
        },
    };

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

/// Spawn a tokio task that runs one tool call and streams its events on `tx`.
///
/// Inside, `tokio::select!` races the tool's future against the progress
/// channel — this guarantees progress events are forwarded the moment the
/// tool emits them, rather than after the tool finishes.
fn spawn_live_tool(
    prepared: PreparedCall,
    tools: ToolRegistry,
    config: AgentConfig,
    tx: tokio::sync::mpsc::UnboundedSender<(usize, ToolExecutionStreamItem)>,
) {
    tokio::spawn(async move {
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
                Err(err) => ToolResult {
                    tool_call_id: call.id.clone(),
                    name: call.name.clone(),
                    content: json_error_payload("tool_not_found", err.to_string()),
                    is_error: true,
                },
            }
        };
        tokio::pin!(result_task);

        let result = loop {
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

        let _ = tx.send((
            index,
            ToolExecutionStreamItem::Event(ToolExecutionEvent::ToolCompleted {
                tool_call_id: result.tool_call_id.clone(),
                name: result.name.clone(),
                is_error: result.is_error,
            }),
        ));
        let _ = tx.send((index, ToolExecutionStreamItem::Result(result)));
    });
}

/// Run a tool through the full validate → permission → invoke pipeline.
///
/// All failure modes (validation, permission, execution) are turned into
/// `is_error: true` [`ToolResult`]s rather than propagating — the model needs
/// a tool result for every tool call it made so it can react and recover.
async fn invoke_existing_tool(
    call: ToolCall,
    tool: Arc<dyn crate::tool::Tool>,
    context: ToolContext,
    config: &AgentConfig,
) -> ToolResult {
    match tool.validate(&call.arguments, &context).await {
        Ok(()) => {
            // The global permission engine wins if it has a rule for this
            // call; otherwise we ask the tool itself.
            let engine_decision = config
                .permission_engine
                .as_ref()
                .and_then(|engine| engine.evaluate_call(&call.name, &call.arguments, &context.cwd));
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

            match permission {
                Ok(PermissionDecision::Allow) => {
                    match tool.invoke(call.arguments.clone(), context).await {
                        Ok(output) => ToolResult {
                            tool_call_id: call.id,
                            name: call.name,
                            content: output.content,
                            is_error: false,
                        },
                        Err(err) => ToolResult {
                            tool_call_id: call.id,
                            name: call.name.clone(),
                            content: json_error_payload("execution_error", err.to_string()),
                            is_error: true,
                        },
                    }
                }
                Ok(PermissionDecision::Deny { reason }) => ToolResult {
                    tool_call_id: call.id,
                    name: call.name.clone(),
                    content: json_error_payload("permission_denied", reason),
                    is_error: true,
                },
                Ok(PermissionDecision::Ask { reason }) => ToolResult {
                    tool_call_id: call.id,
                    name: call.name.clone(),
                    content: json_error_payload("permission_required", reason),
                    is_error: true,
                },
                Err(err) => ToolResult {
                    tool_call_id: call.id,
                    name: call.name.clone(),
                    content: json_error_payload("permission_error", err.to_string()),
                    is_error: true,
                },
            }
        }
        Err(err) => ToolResult {
            tool_call_id: call.id,
            name: call.name.clone(),
            content: json_error_payload("validation_error", err.to_string()),
            is_error: true,
        },
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
