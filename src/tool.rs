//! Tool abstraction — pluggable side-effectful capabilities exposed to the model.
//!
//! A [`Tool`] declares its JSON schema via [`Tool::definition`] and runs in
//! [`Tool::invoke`]. The default implementations of [`validate`](Tool::validate)
//! and [`check_permission`](Tool::check_permission) accept everything;
//! override them to enforce input shape or per-call gating.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use crate::error::AgentError;
use crate::message::Message;

/// Public-facing description of a tool sent to the model.
///
/// `input_schema` is JSON Schema; providers translate it into their native
/// tool-spec format (Anthropic `input_schema`, OpenAI `function.parameters`).
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Successful outcome of [`Tool::invoke`].
///
/// Always JSON — wrap free text via [`ToolOutput::text`] for the common case.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: Value,
}

impl ToolOutput {
    /// Wrap a plain text result as `{ "text": "…" }`.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: json!({ "text": text.into() }),
        }
    }

    /// Wrap an arbitrary JSON value as the tool output.
    pub fn json(content: Value) -> Self {
        Self { content }
    }
}

/// Streaming progress update emitted from inside a long-running tool.
///
/// Sent through [`ToolContext::progress`] so the runtime can surface
/// intermediate state to its callers without waiting for the tool to finish.
#[derive(Debug, Clone)]
pub struct ToolProgress {
    pub tool_call_id: Option<String>,
    pub message: String,
    pub data: Option<Value>,
}

/// Metadata captured when a file is read through the built-in `Read` tool.
///
/// Mutating file tools use this to reject stale writes: if the file changed
/// after the model read it, the model must read it again before editing.
#[derive(Debug, Clone)]
pub struct FileReadRecord {
    pub content: String,
    pub timestamp_ms: u128,
    pub is_partial_view: bool,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

/// Shared per-session file-read cache.
pub type FileReadState = Arc<Mutex<HashMap<PathBuf, FileReadRecord>>>;

/// How a tool should respond when an interruption is requested.
///
/// Currently informational — used by hosts that implement Ctrl-C-style cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptBehavior {
    /// Wait for the in-flight call to complete before honouring the interrupt.
    Block,
    /// Abort the in-flight call immediately.
    Cancel,
}

/// Result of a per-call permission check.
///
/// Tools may delegate to the runtime's [`PermissionEngine`](crate::PermissionEngine)
/// (see [`AgentConfig::permission_engine`](crate::AgentConfig::permission_engine))
/// or implement their own policy in [`Tool::check_permission`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Proceed with the call.
    Allow,
    /// Refuse the call; the model receives an error result.
    Deny { reason: String },
    /// Defer to the host (typically a human approval prompt).
    Ask { reason: String },
}

/// Per-invocation context handed to a tool.
///
/// Cloning this struct is cheap-ish but `messages` is the full conversation —
/// avoid retaining the whole context inside long-lived state.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub session_id: String,
    pub turn_id: u64,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    /// Snapshot of the conversation up to (but not including) this tool call.
    pub messages: Vec<Message>,
    /// Channel for emitting [`ToolProgress`] events while the tool runs.
    pub progress: Option<mpsc::UnboundedSender<ToolProgress>>,
    /// Per-session file-read cache used by filesystem tools to prevent stale writes.
    pub read_file_state: FileReadState,
}

/// A tool that can be invoked by the agent.
///
/// Implementations must provide at least [`definition`](Tool::definition) and
/// [`invoke`](Tool::invoke). The remaining methods have sensible defaults.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Describe the tool's name, prose description, and JSON-schema input.
    fn definition(&self) -> ToolDefinition;

    /// Backwards-compatible alternate names accepted by the runtime.
    ///
    /// Aliases are *not* sent to the model; they only let older transcripts or
    /// callers invoke renamed tools.
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// Validate raw arguments before the permission check runs.
    ///
    /// Default: accept anything.
    async fn validate(&self, _arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        Ok(())
    }

    /// Decide whether the call is allowed, denied, or needs human approval.
    ///
    /// The runtime first consults the global [`PermissionEngine`](crate::PermissionEngine)
    /// (if configured) and only falls back to this method when no rule matches.
    /// Default: allow.
    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Allow)
    }

    /// How the tool wants to be interrupted.
    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Block
    }

    /// Whether the tool is safe to run concurrently with other invocations.
    ///
    /// Side-effect-free / read-only tools should return `true` so the runtime
    /// can batch them. Default: `false` (serial).
    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        false
    }

    /// Execute the tool. Errors are surfaced as `is_error: true` tool results
    /// rather than aborting the turn, so the model can try to recover.
    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError>;
}

/// Name-indexed collection of [`Tool`]s available to the agent.
///
/// `Clone` is cheap — `Arc<dyn Tool>` values are shared.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    canonical_names: Vec<String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool. A later registration with the same name overrides the earlier one.
    pub fn register<T>(&mut self, tool: T)
    where
        T: Tool + 'static,
    {
        let definition = tool.definition();
        let name = definition.name.clone();
        let aliases = tool.aliases();
        let tool = Arc::new(tool);
        self.tools.insert(name.clone(), tool.clone());
        self.canonical_names.push(name);
        for alias in aliases {
            self.tools.insert((*alias).to_string(), tool.clone());
        }
    }

    /// Collect [`ToolDefinition`]s for every registered tool — sent to the provider on each turn.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .filter(|(name, _)| {
                self.canonical_names
                    .iter()
                    .any(|canonical| canonical == *name)
            })
            .map(|(_, tool)| tool.definition())
            .collect::<Vec<_>>()
    }

    /// Look up a tool by name. Returns [`AgentError::ToolNotFound`] if absent.
    pub fn get(&self, name: &str) -> Result<Arc<dyn Tool>, AgentError> {
        self.tools
            .get(name)
            .cloned()
            .ok_or_else(|| AgentError::ToolNotFound(name.to_string()))
    }
}
