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
pub mod approval;
pub mod bash_security;
pub mod compaction;
pub mod config;
pub mod error;
pub mod executor;
pub mod hooks;
pub mod mcp;
pub mod memory;
pub mod message;
pub mod metrics;
pub mod mock;
pub mod permissions;
pub mod prompt;
pub mod provider;
pub mod runtime;
pub mod skills;
pub mod storage;
pub mod subagent;
pub mod tasks;
pub mod tokens;
pub mod tool;
pub mod tools;

// Approval — asynchronous human-in-the-loop gating for tool calls.
pub use approval::{
    ApprovalDecision, ApprovalHandler, ApprovalRequest, AutoDenyHandler, FixedDecisionHandler,
};
// Compaction — history- and message-level shrinking strategies.
pub use compaction::{CompactionStrategy, SummaryCompaction};
// Configuration — the session config aggregate and the token-budget knob.
pub use config::{AgentConfig, TokenBudget};
// Errors — the single failure type used across the crate.
pub use error::{AgentError, ProviderError};
// Tool executor — direct entry points for callers that bypass the turn loop.
pub use executor::{ToolExecutionEvent, ToolExecutionOutput, execute_tool_calls};
// Hooks — registry + per-phase hook trait + metadata types.
pub use hooks::{Hook, HookCondition, HookContext, HookEntry, HookPhase, HookRegistry};
// Message model — the lingua franca between session, provider, and tools.
pub use message::{ContentBlock, Message, Role, TextBlock, ThinkingBlock, ToolCall, ToolResult};
// Memory — persistent cross-session agent memory.
pub use memory::ProfileManager;
pub use memory::{MemoryCategory, MemoryEntry, MemoryFormat, MemoryStatus, MemoryStore};
pub use memory::{
    MemoryEditTool, MemoryGrepTool, MemoryReadTool, MemoryStatusTool, MemoryWriteTool,
};
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
// Skills — user-defined slash-commands loaded from markdown files.
pub use skills::{Skill, SkillArg, SkillLoader, SkillRegistry, SkillSource};
// Storage — persistence backends for saving and resuming sessions.
pub use storage::{JsonlStorage, NoopStorage, Storage};
// Subagent — nested agent run exposed as a tool and Fork concurrent-execution engine.
pub use subagent::{ForkExecution, ForkLens, ForkResult, ForkShared, SubagentTool, Synapse};
// Tasks — task management system with tracking, persistence, and tool integration.
pub use tasks::{Task, TaskManager, TaskStatus};
// MCP — stdio-based Model Context Protocol client + manager + bridge.
pub use mcp::{McpClient, McpManager, McpTool, McpToolBridge};
// Prompt system — modular, cache-aware construction of the system prompt.
pub use prompt::{
    CwdSection, DateSection, GitStatusSection, IdentitySection, McpSection, MemorySection,
    ProfileSection, PromptAssembly, PromptSection, PromptStability, SafetySection, SkillsSection,
    TaskGuidanceSection, ToneStyleSection, ToolUsageSection, ToolsSection,
};
// Tool abstraction — the trait every callable capability implements, plus its registry.
pub use tool::validate::{ValidationError, ValidationResult, validate_arguments};
pub use tool::{
    InterruptBehavior, PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput,
    ToolProgress, ToolRegistry,
};
// Built-in tools — filesystem, shell, search, web, user interaction.
pub use tools::{
    AskUserQuestionTool, FileEditTool, FileReadTool, FileWriteTool, GlobTool, GrepTool, ShellTool,
    SkillTool, TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool, WebFetchTool,
    WebSearchTool, register_core_tools, register_task_tools,
};
