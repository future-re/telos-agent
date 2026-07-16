use crate::config::AgentConfig;
use crate::diagnostics::{ToolDiagnosticsSink, ToolFailureEvent, ToolFailureKind};
use crate::error::AgentError;
use crate::model::message::ToolCall;
use crate::tools::api::{
    FileReadState, Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry,
};
use crate::tools::executor::{ToolExecutionStreamItem, execute_tool_calls_stream};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;

struct ProbeTool {
    current: Arc<AtomicUsize>,
    max: Arc<AtomicUsize>,
    delay: std::time::Duration,
}

#[async_trait]
impl Tool for ProbeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "probe".into(),
            description: "concurrency probe".into(),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        true
    }

    async fn invoke(
        &self,
        _arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let prev = self.current.fetch_add(1, Ordering::SeqCst);
        self.max.fetch_max(prev + 1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        self.current.fetch_sub(1, Ordering::SeqCst);
        Ok(ToolOutput::text("ok"))
    }
}

struct PanickingTool;

#[async_trait]
impl Tool for PanickingTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "panic".into(),
            description: "panics on invoke".into(),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        true
    }

    async fn invoke(
        &self,
        _arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        panic!("intentional tool panic");
    }
}

#[derive(Debug)]
struct FailingTool;

#[async_trait]
impl Tool for FailingTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Fail".into(),
            description: "fails on invoke".into(),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    async fn invoke(
        &self,
        _arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        Err(AgentError::ToolExecution { tool: "Fail".into(), message: "simulated failure".into() })
    }
}

#[derive(Debug, Default)]
struct MemoryDiagnosticsSink {
    events: Mutex<Vec<ToolFailureEvent>>,
}

#[async_trait]
impl ToolDiagnosticsSink for MemoryDiagnosticsSink {
    async fn record(&self, event: ToolFailureEvent) -> Result<(), AgentError> {
        self.events.lock().await.push(event);
        Ok(())
    }
}

fn test_config(limit: usize) -> AgentConfig {
    AgentConfig { tool_concurrency_limit: limit, ..Default::default() }
}

fn make_call(id: &str, name: &str) -> ToolCall {
    ToolCall { id: id.into(), name: name.into(), arguments: serde_json::json!({}) }
}

fn empty_read_file_state() -> FileReadState {
    Arc::new(Mutex::new(std::collections::HashMap::new()))
}

async fn collect_results(
    stream: impl futures_core::stream::Stream<Item = ToolExecutionStreamItem>,
) -> Vec<crate::model::message::ToolResult> {
    let mut stream = Box::pin(stream);
    let mut results = Vec::new();
    while let Some(item) = stream.next().await {
        if let ToolExecutionStreamItem::Result(result) = item {
            results.push(result);
        }
    }
    results
}

#[tokio::test]
async fn executor_records_tool_execution_failure() {
    let sink = Arc::new(MemoryDiagnosticsSink::default());
    let mut registry = ToolRegistry::new();
    registry.register(FailingTool);

    let config = AgentConfig { tool_diagnostics: Some(sink.clone()), ..test_config(1) };

    let stream = execute_tool_calls_stream(
        vec![make_call("call-1", "Fail")],
        &registry,
        &config,
        "session-1",
        1,
        Arc::new(vec![]),
        empty_read_file_state(),
    );
    let results = collect_results(stream).await;

    assert_eq!(results.len(), 1);
    assert!(results[0].is_error);
    let events = sink.events.lock().await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tool_name, "Fail");
    assert_eq!(events[0].failure_kind, ToolFailureKind::ExecutionError);
    assert_eq!(events[0].session_id, "session-1");
    assert_eq!(events[0].turn_id, 1);
}

#[tokio::test]
async fn concurrency_safe_tools_run_in_parallel() {
    let current = Arc::new(AtomicUsize::new(0));
    let max = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProbeTool {
        current: Arc::clone(&current),
        max: Arc::clone(&max),
        delay: std::time::Duration::from_millis(50),
    });

    let config = test_config(3);
    let calls = (0..5).map(|i| make_call(&format!("call-{i}"), "probe")).collect();

    let stream = execute_tool_calls_stream(
        calls,
        &registry,
        &config,
        "s",
        1,
        Arc::new(vec![]),
        empty_read_file_state(),
    );
    let results = collect_results(stream).await;

    assert_eq!(results.len(), 5);
    for result in &results {
        assert!(!result.is_error, "probe should succeed: {result:?}");
    }
    assert!(
        max.load(Ordering::SeqCst) >= 2,
        "expected concurrent execution, got max={}",
        max.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn panicking_tool_is_isolated_and_other_tools_complete() {
    let mut registry = ToolRegistry::new();
    registry.register(PanickingTool);
    registry.register(ProbeTool {
        current: Arc::new(AtomicUsize::new(0)),
        max: Arc::new(AtomicUsize::new(0)),
        delay: std::time::Duration::from_millis(5),
    });

    let config = test_config(3);
    let calls = vec![make_call("c1", "panic"), make_call("c2", "probe"), make_call("c3", "probe")];

    let stream = execute_tool_calls_stream(
        calls,
        &registry,
        &config,
        "s",
        1,
        Arc::new(vec![]),
        empty_read_file_state(),
    );
    let results = collect_results(stream).await;

    assert_eq!(results.len(), 3);
    assert!(results[0].is_error);
    assert!(results[0].content.to_string().contains("panic"));
    assert!(!results[1].is_error);
    assert!(!results[2].is_error);
}
