//! Tool execution engine with batching and streaming support.
//!
//! Tools marked [`is_concurrency_safe`](crate::Tool::is_concurrency_safe) are grouped into
//! concurrent batches; others run sequentially. Batches preserve the original
//! call order in their results so the model always sees deterministic output.

pub use stream::execute_tool_calls_stream;
pub use sync::execute_tool_calls;
pub use types::{ToolExecutionEvent, ToolExecutionOutput, ToolExecutionStreamItem};

mod batch;
mod invoke;
mod stream;
mod sync;
#[cfg(test)]
mod tests;
mod types;
