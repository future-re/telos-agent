//! Types for the tool execution engine.

use crate::message::{ToolCall, ToolResult};
use crate::tool::ToolContext;
use serde_json::Value;

/// Lifecycle event emitted by the executor for one tool invocation.
#[derive(Debug, Clone)]
pub enum ToolExecutionEvent {
    /// Emitted once when the tool starts (after permission, before `invoke`).
    ToolStarted { tool_call_id: String, name: String, detail: String },
    /// Streaming progress update from inside the tool.
    ToolProgress {
        tool_call_id: Option<String>,
        name: String,
        message: String,
        data: Option<Value>,
    },
    /// Emitted once when the tool finishes (success or error).
    ToolCompleted { tool_call_id: String, name: String, is_error: bool },
    /// A tool call has been suspended pending human approval.
    ApprovalRequested { tool_call_id: String, name: String, reason: String },
    /// Human approval has been resolved for a suspended tool call.
    ApprovalResolved { tool_call_id: String, name: String, decision: String },
}

/// Buffered output of [`execute_tool_calls`](crate::execute_tool_calls) — events in chronological order,
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
pub(crate) struct PreparedCall {
    /// Position in the original call list — used to restore deterministic order after concurrent execution.
    pub(crate) index: usize,
    pub(crate) call: ToolCall,
    pub(crate) context: ToolContext,
}

/// A contiguous run of calls that can either all run in parallel (when
/// `concurrency_safe`) or must run sequentially.
#[derive(Debug, Clone)]
pub(crate) struct Batch {
    pub(crate) concurrency_safe: bool,
    pub(crate) calls: Vec<PreparedCall>,
}

/// Items yielded by the streaming executor — lifecycle events as they
/// happen, and final results once each call completes.
#[derive(Debug, Clone)]
pub enum ToolExecutionStreamItem {
    /// An [`ToolExecutionEvent`] — informational; may be emitted out of call order.
    Event(ToolExecutionEvent),
    /// A finished tool's [`ToolResult`] — emitted in the original call order at end of batch.
    Result(ToolResult),
}
