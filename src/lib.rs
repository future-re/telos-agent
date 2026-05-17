//! Tiny Agent Core — a lightweight, provider-agnostic agent runtime.
//!
//! The crate provides:
//! - [`AgentSession`] — the main turn loop (model → tools → model)
//! - [`Tool`] trait and [`ToolRegistry`] — pluggable tool system
//! - [`ModelProvider`] trait — pluggable LLM backends (Anthropic, OpenAI)
//! - [`Hook`] system — intercept assistant messages (post-sampling, stop)
//! - Context compaction — token-budget-aware summarization
//! - Permission engine — rule-based tool allow/deny
//! - JSONL session storage — save/resume agent state

pub mod compaction;
pub mod config;
pub mod error;
pub mod executor;
pub mod hooks;
pub mod message;
pub mod mock;
pub mod permissions;
pub mod provider;
pub mod runtime;
pub mod storage;
pub mod subagent;
pub mod tool;
pub mod tools;

pub use compaction::{CompactionStrategy, SummaryCompaction};
pub use config::{AgentConfig, TokenBudget};
pub use error::AgentError;
pub use executor::{ToolExecutionEvent, ToolExecutionOutput, execute_tool_calls};
pub use hooks::{Hook, HookContext, HookPhase, HookRegistry};
pub use message::{ContentBlock, Message, Role, TextBlock, ToolCall, ToolResult};
pub use mock::MockProvider;
pub use permissions::{PermissionEngine, PermissionRule, RuleDecision};
pub use provider::{
    AnthropicConfig, AnthropicProvider, CompletionRequest, CompletionResponse, ModelProvider,
    OpenAIConfig, OpenAIProvider, ProviderEvent, StopReason, TokenUsage,
};
pub use runtime::{AgentSession, TurnEvent, TurnResult};
pub use storage::{JsonlStorage, NoopStorage, Storage};
pub use subagent::SubagentTool;
pub use tool::{
    InterruptBehavior, PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput,
    ToolProgress, ToolRegistry,
};
pub use tools::{
    FileEditTool, FileReadTool, FileWriteTool, GlobTool, GrepTool, ShellTool, register_core_tools,
};
