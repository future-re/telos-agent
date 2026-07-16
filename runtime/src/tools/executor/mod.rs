//! Tool execution engine with batching and streaming support.
//!
//! Tools marked [`is_concurrency_safe`](crate::Tool::is_concurrency_safe) are grouped into
//! concurrent batches; others run sequentially. Batches preserve the original
//! call order in their results so the model always sees deterministic output.

pub(crate) use invoke::tool_result_detail;
pub use stream::execute_tool_calls_stream;
pub use types::{ToolExecutionEvent, ToolExecutionOutput, ToolExecutionStreamItem};

mod batch;
mod invoke;
mod stream;
#[cfg(test)]
mod tests;
mod types;
