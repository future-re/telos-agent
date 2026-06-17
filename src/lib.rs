//! Tiny Agent Core — a lightweight, provider-agnostic agent runtime.
//!
//! The crate provides:
//! - [`AgentSession`] — the main turn loop (model → tools → model)
//! - [`Tool`] trait and [`ToolRegistry`] — pluggable tool system
//! - [`ModelProvider`] trait — pluggable LLM backends (DeepSeek, Kimi)
//! - [`Hook`] system — intercept assistant messages (post-sampling, stop)
//! - Context compaction — token-budget-aware summarization
//! - Permission engine — rule-based tool allow/deny
//! - JSONL session storage — save/resume agent state
//!
//! # Quick start
//!
//! Build an [`AgentConfig`], create a [`ToolRegistry`], pick a
//! [`ModelProvider`], then drive a turn via [`AgentSession::run_turn`] for a
//! blocking call or [`AgentSession::run_turn_stream`] for a UI-friendly event
//! stream.

// Module declarations — public so downstream crates can name internal types directly.
pub mod compaction;
pub mod config;
pub mod error;
pub mod executor;
pub mod hooks;
pub mod message;
pub mod metrics;
pub mod mock;
pub mod permissions;
pub mod provider;
pub mod runtime;
pub mod storage;
pub mod subagent;
pub mod tool;
pub mod tools;

// Compaction — history- and message-level shrinking strategies.
pub use compaction::{CompactionStrategy, SummaryCompaction};
// Configuration — the session config aggregate and the token-budget knob.
pub use config::{AgentConfig, TokenBudget};
// Errors — the single failure type used across the crate.
pub use error::AgentError;
// Tool executor — direct entry points for callers that bypass the turn loop.
pub use executor::{ToolExecutionEvent, ToolExecutionOutput, execute_tool_calls};
// Hooks — registry + per-phase hook trait.
pub use hooks::{Hook, HookContext, HookPhase, HookRegistry};
// Message model — the lingua franca between session, provider, and tools.
pub use message::{ContentBlock, Message, Role, TextBlock, ToolCall, ToolResult};
// Metrics — session-level counters accumulated by the runtime.
pub use metrics::SessionMetrics;
// Test helper — pre-canned [`ModelProvider`] for unit tests.
pub use mock::MockProvider;
// Permissions — rule-based gating of tool calls.
pub use permissions::{PermissionEngine, PermissionRule, RuleDecision};
// Provider — the trait downstream LLM backends implement, plus built-in impls.
pub use provider::{
    CompletionRequest, CompletionResponse, DeepSeekConfig, DeepSeekProvider, ErasedProvider,
    KimiConfig, KimiProvider, ModelProvider, ProviderEvent, StopReason, TokenUsage,
};
// Runtime — the agent session and the streaming turn loop.
pub use runtime::{AgentSession, TurnEvent, TurnResult};
// Storage — persistence backends for saving and resuming sessions.
pub use storage::{JsonlStorage, NoopStorage, Storage};
// Subagent — nested agent run exposed as a tool.
pub use subagent::SubagentTool;
// Tool abstraction — the trait every callable capability implements, plus its registry.
pub use tool::{
    InterruptBehavior, PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput,
    ToolProgress, ToolRegistry,
};
// Built-in tools — filesystem, shell, search.
pub use tools::{
    FileEditTool, FileReadTool, FileWriteTool, GlobTool, GrepTool, ShellTool, register_core_tools,
};
