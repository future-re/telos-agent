//! Tool abstraction — pluggable side-effectful capabilities exposed to the model.
//!
//! A [`Tool`] declares its JSON schema via [`Tool::definition`] and runs in
//! [`Tool::invoke`]. The default implementations of [`validate`](Tool::validate)
//! and [`check_permission`](Tool::check_permission) accept everything;
//! override them to enforce input shape or per-call gating.

use async_trait::async_trait;
use jsonschema::Validator;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use crate::error::AgentError;
use crate::message::Message;

pub mod validate;

/// Public-facing description of a tool sent to the model.
///
/// `input_schema` is JSON Schema; providers translate it into their native
/// tool-spec format (OpenAI-compatible `function.parameters`).
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
        Self { content: json!({ "text": text.into() }) }
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
/// Cloning this struct is cheap because the conversation snapshot is shared
/// via [`Arc`]. Avoid retaining the whole context inside long-lived state.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub session_id: String,
    pub turn_id: u64,
    /// Current parent tool call id, when this context is attached to a model tool call.
    pub tool_call_id: Option<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    /// Snapshot of the conversation up to (but not including) this tool call.
    pub messages: Arc<Vec<Message>>,
    /// Channel for emitting [`ToolProgress`] events while the tool runs.
    pub progress: Option<mpsc::UnboundedSender<ToolProgress>>,
    /// Per-session file-read cache used by filesystem tools to prevent stale writes.
    pub read_file_state: FileReadState,
    /// Optional per-call timeout. The executor will cancel `invoke` if it
    /// exceeds this duration and return an `is_error: true` result.
    pub timeout: Option<std::time::Duration>,
    /// Maximum bytes the built-in file tools will read from a single file.
    pub max_file_read_bytes: usize,
}

impl ToolContext {
    /// Create a minimal context for unit tests.
    #[cfg(test)]
    pub fn dummy() -> Self {
        Self {
            session_id: "test-session".into(),
            turn_id: 0,
            tool_call_id: None,
            cwd: std::path::PathBuf::from("."),
            env: std::collections::HashMap::new(),
            messages: std::sync::Arc::new(Vec::new()),
            progress: None,
            read_file_state: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            timeout: None,
            max_file_read_bytes: 50 * 1024 * 1024,
        }
    }
}

/// A tool that can be invoked by the agent.
///
/// Implementations must provide at least [`definition`](Tool::definition) and
/// [`invoke`](Tool::invoke). The remaining methods have sensible defaults.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Describe the tool's name, prose description, and JSON-schema input.
    fn definition(&self) -> ToolDefinition;

    /// Optional detailed usage instructions injected into the system prompt.
    /// Return `None` if the tool has no extra behavioral guidance.
    fn prompt_text(&self) -> Option<&'static str> {
        None
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
    /// Pre-compiled JSON Schema validators keyed by canonical tool name.
    validators: HashMap<String, Arc<Validator>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Iterate all registered tools as `(canonical_name, tool)` pairs.
    /// The `Arc` is cloned; the underlying tool is shared.
    pub fn iter(&self) -> impl Iterator<Item = (&String, Arc<dyn Tool>)> + '_ {
        self.tools.iter().map(|(name, tool)| (name, Arc::clone(tool)))
    }

    /// Register a tool. A later registration with the same name overrides the earlier one.
    pub fn register<T>(&mut self, tool: T)
    where
        T: Tool + 'static,
    {
        let definition = tool.definition();
        let name = definition.name.clone();
        let tool = Arc::new(tool);
        let is_override = self.tools.insert(name.clone(), tool.clone()).is_some();
        if !is_override {
            self.canonical_names.push(name.clone());
        }
        // Pre-compile the JSON Schema validator so every invocation does not
        // pay the compilation cost again. Invalid schemas are treated as a
        // programming error and fail fast.
        match Validator::new(&definition.input_schema) {
            Ok(validator) => {
                self.validators.insert(name.clone(), Arc::new(validator));
            }
            Err(err) => {
                panic!("tool `{}` has an invalid input schema: {err}", name);
            }
        }
    }

    /// Collect [`ToolDefinition`]s for every registered tool — sent to the provider on each turn.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions = self
            .canonical_names
            .iter()
            .filter_map(|name| self.tools.get(name).map(|tool| tool.definition()))
            .collect::<Vec<_>>();
        definitions.sort_by(|a, b| a.name.cmp(&b.name));
        definitions
    }

    /// Return a registry containing only tools allowed by `allowed` and not denied by `denied`.
    ///
    /// Empty `allowed` means all tools. A literal `"*"` also means all tools.
    pub fn filtered(&self, allowed: &[String], denied: &[String]) -> Self {
        let allowed_all = allowed.is_empty() || allowed.iter().any(|name| name == "*");

        let mut next = ToolRegistry::new();
        for canonical in &self.canonical_names {
            if !allowed_all && !allowed.contains(canonical) {
                continue;
            }
            if denied.contains(canonical) {
                continue;
            }
            if let Some(tool) = self.tools.get(canonical) {
                next.tools.insert(canonical.clone(), tool.clone());
                next.canonical_names.push(canonical.clone());
                if let Some(validator) = self.validators.get(canonical) {
                    next.validators.insert(canonical.clone(), validator.clone());
                }
            }
        }
        next
    }

    /// Look up a tool by name. Returns [`AgentError::ToolNotFound`] if absent.
    pub fn get(&self, name: &str) -> Result<Arc<dyn Tool>, AgentError> {
        self.tools.get(name).cloned().ok_or_else(|| AgentError::ToolNotFound(name.to_string()))
    }

    /// Validate `arguments` against the cached JSON Schema validator for the
    /// tool named `name`. Returns [`AgentError::ToolNotFound`] if the tool is
    /// not registered, or [`AgentError::Validation`] if the arguments fail.
    pub fn validate_arguments(&self, name: &str, arguments: &Value) -> Result<(), AgentError> {
        if !self.tools.contains_key(name) {
            return Err(AgentError::ToolNotFound(name.to_string()));
        }

        let validator =
            self.validators.get(name).cloned().expect("validator missing for registered tool");

        let mut errors = Vec::new();
        for err in validator.iter_errors(arguments) {
            errors.push(format!("{}: {}", err.instance_path, err));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(AgentError::Validation(format!(
                "tool `{name}` arguments failed schema validation: {}",
                errors.join("; ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;

    struct FakeTool {
        def: ToolDefinition,
    }

    impl FakeTool {
        fn new(name: &str) -> Self {
            Self {
                def: ToolDefinition {
                    name: name.into(),
                    description: "test".into(),
                    input_schema: json!({"type": "object"}),
                },
            }
        }
    }

    #[async_trait]
    impl Tool for FakeTool {
        fn definition(&self) -> ToolDefinition {
            self.def.clone()
        }
        async fn invoke(
            &self,
            _arguments: Value,
            _context: ToolContext,
        ) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }

    #[test]
    fn registry_lookup_by_name() {
        let mut registry = ToolRegistry::new();
        registry.register(FakeTool::new("Read"));
        assert!(registry.get("Read").is_ok());
    }

    #[test]
    fn registry_get_unknown_tool_returns_tool_not_found() {
        let registry = ToolRegistry::new();
        assert!(matches!(registry.get("nonexistent"), Err(AgentError::ToolNotFound(_))));
    }

    #[test]
    fn registry_canonical_names_do_not_duplicate() {
        let mut registry = ToolRegistry::new();
        registry.register(FakeTool::new("A"));
        registry.register(FakeTool::new("B"));
        let defs = registry.definitions();
        assert_eq!(defs.len(), 2);
    }

    #[test]
    fn registry_re_register_overrides() {
        let mut registry = ToolRegistry::new();
        registry.register(FakeTool::new("X"));
        registry.register(FakeTool::new("X"));
        let defs = registry.definitions();
        assert_eq!(defs.len(), 1);
    }
}
