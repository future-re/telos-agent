//! Tests for the tool execution engine.

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::executor::execute_tool_calls;
use crate::message::ToolCall;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Probe tool that tracks how many invocations run at the same time.
#[derive(Debug)]
struct ConcurrencyProbe {
    current: Arc<AtomicUsize>,
    max: Arc<AtomicUsize>,
    delay: std::time::Duration,
}

#[async_trait]
impl Tool for ConcurrencyProbe {
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

/// Tool that always panics inside `invoke`.
#[derive(Debug)]
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

fn test_config(limit: usize) -> AgentConfig {
    AgentConfig { tool_concurrency_limit: limit, ..Default::default() }
}

fn make_call(id: &str, name: &str) -> ToolCall {
    ToolCall { id: id.into(), name: name.into(), arguments: serde_json::json!({}) }
}

#[tokio::test]
async fn concurrency_safe_tools_run_in_parallel() {
    let current = Arc::new(AtomicUsize::new(0));
    let max = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ConcurrencyProbe {
        current: Arc::clone(&current),
        max: Arc::clone(&max),
        delay: std::time::Duration::from_millis(50),
    });

    let config = test_config(3);
    let calls = (0..5).map(|i| make_call(&format!("call-{i}"), "probe")).collect();

    let output = execute_tool_calls(
        calls,
        &registry,
        &config,
        "s",
        1,
        Arc::new(vec![]),
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
    )
    .await;

    assert_eq!(output.results.len(), 5);
    for result in &output.results {
        assert!(!result.is_error, "probe should succeed: {result:?}");
    }
    // With a concurrency limit of 3 and five 50ms sleeps, we should observe
    // at least 2 running concurrently; a purely serial run would be 1.
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
    registry.register(ConcurrencyProbe {
        current: Arc::new(AtomicUsize::new(0)),
        max: Arc::new(AtomicUsize::new(0)),
        delay: std::time::Duration::from_millis(5),
    });

    let config = test_config(3);
    let calls = vec![make_call("c1", "panic"), make_call("c2", "probe"), make_call("c3", "probe")];

    let output = execute_tool_calls(
        calls,
        &registry,
        &config,
        "s",
        1,
        Arc::new(vec![]),
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
    )
    .await;

    assert_eq!(output.results.len(), 3);
    assert!(output.results[0].is_error);
    assert!(output.results[0].content.to_string().contains("panicked"));
    assert!(!output.results[1].is_error);
    assert!(!output.results[2].is_error);
}
