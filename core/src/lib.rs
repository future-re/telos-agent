#![warn(rustdoc::broken_intra_doc_links)]
#![doc(html_root_url = "https://docs.rs/telos_agent/0.1.0")]

//! Core Rust runtime for **telos**.
//!
//! `telos_agent` is a provider-agnostic agent runtime for applications that
//! need the loop:
//!
//! ```text
//! user intent -> model sampling -> tool execution -> result injection -> final answer
//! ```
//!
//! The crate is intended to be embedded by CLIs, desktop apps, servers, and
//! workflow tools. The terminal client (`telos-cli`) and desktop prototype are
//! hosts built on top of this library.
//!
//! # Primary API
//!
//! Most consumers only need these types:
//!
//! - [`AgentSession`] — owns conversation state and drives turns.
//! - [`AgentConfig`] — configures cwd, env, storage, approvals, compaction,
//!   token budgets, hooks, and cancellation.
//! - [`ModelProvider`] — trait implemented by LLM backends.
//! - [`DeepSeekProvider`], [`RoutedProvider`], and [`MockProvider`] — built-in
//!   provider implementations.
//! - [`Tool`] and [`ToolRegistry`] — define and register callable tools.
//! - [`TurnEvent`] — streaming UI/API event surface for assistant deltas,
//!   thinking deltas, tool lifecycle, retries, usage, approvals, and turn
//!   completion.
//! - [`Storage`], [`JsonlStorage`], and [`NoopStorage`] — session persistence.
//!
//! # Quick Start
//!
//! Build an [`AgentConfig`], create a [`ToolRegistry`], pick a
//! [`ModelProvider`], then drive a turn via [`AgentSession::run_turn`] for a
//! blocking call or [`AgentSession::run_turn_stream`] for a UI-friendly event
//! stream.
//!
//! ```rust
//! use telos_agent::{
//!     AgentConfig, AgentError, AgentSession, CompletionResponse, Message,
//!     MockProvider, StopReason, ToolRegistry,
//! };
//!
//! #[tokio::main]
//! async fn main() -> Result<(), AgentError> {
//!     let provider = MockProvider::new(vec![CompletionResponse {
//!         message: Message::assistant("done"),
//!         stop_reason: StopReason::EndTurn,
//!         usage: None,
//!     }]);
//!
//!     let tools = ToolRegistry::new();
//!     let mut session = AgentSession::new(AgentConfig {
//!         base_system_prompt: Some("You are concise.".into()),
//!         ..Default::default()
//!     })?;
//!
//!     let result = session.run_turn(&provider, &tools, "hello").await?;
//!     assert_eq!(result.final_message.text_content(), "done");
//!     Ok(())
//! }
//! ```
//!
//! # Streaming Turns
//!
//! Hosts that render live UI should use [`AgentSession::run_turn_stream`]. It
//! yields [`TurnEvent`] values in order, so the host can update assistant text,
//! thinking text, tool activity, approval prompts, token usage, and completion
//! state without parsing model messages directly.
//!
//! # Extension Points
//!
//! - Implement [`ModelProvider`] to add a new model backend.
//! - Implement [`Tool`] to expose a new capability.
//! - Implement [`ApprovalHandler`] for host-specific approval UI.
//! - Implement [`Hook`] to react at runtime phases.
//! - Implement [`Storage`] for a custom persistence backend.
//! - Use [`McpManager`] and [`McpToolBridge`] to bridge MCP servers into the
//!   tool registry.
//!
//! # Public Module Map
//!
//! The crate root re-exports the supported high-level API. Public modules are
//! also available for advanced callers that need lower-level types.
//!
//! | Module | Purpose |
//! |---|---|
//! | [`runtime`] | [`AgentSession`], [`TurnEvent`], and turn orchestration. |
//! | [`provider`] | Provider trait, DeepSeek provider, routed provider, request/response types. |
//! | [`tool`] / [`tools`] | Tool trait, registry, executor-facing context, and built-in tools. |
//! | [`approval`] / [`permissions`] | Human approval and rule-based tool gating. |
//! | [`prompt`] / [`skills`] | Prompt assembly and markdown skill loading. |
//! | [`memory`] / [`tasks`] | Persistent memory and task tracking. |
//! | [`mcp`] / [`plugin`] / [`subagent`] | External tools, plugin loading, and nested agents. |
//! | [`storage`] / [`compaction`] | Session persistence and context-window management. |
//! | [`diagnostics`] / [`metrics`] | Sanitized tool failure records and session counters. |

/// Human-in-the-loop approval decisions for tool calls.
pub mod approval;
/// Bash command safety analysis used by shell permissions.
pub mod bash_security;
/// Lightweight repository index used by code-search tools.
pub mod code_index;
/// Context-window and tool-result compaction strategies.
pub mod compaction;
/// Runtime configuration and cancellation state.
pub mod config;
/// Sanitized tool failure diagnostics.
pub mod diagnostics;
/// Error types shared across runtime, tools, and providers.
pub mod error;
/// Tool-call executor for direct and turn-loop use.
pub mod executor;
/// Runtime hook registry and hook phases.
pub mod hooks;
/// Model Context Protocol client and tool bridge.
pub mod mcp;
/// Persistent memory store and profile management.
pub mod memory;
/// Conversation message model.
pub mod message;
/// Runtime metrics accumulated by sessions.
pub mod metrics;
/// Mock model provider for tests and demos.
pub mod mock;
/// Rule-based permission engine.
pub mod permissions;
/// Plugin manifest, marketplace, registry, and tool loading.
pub mod plugin;
/// PowerShell command safety analysis used by PowerShell permissions.
pub mod powershell_security;
/// System-prompt section and assembly system.
pub mod prompt;
/// Model provider trait and built-in providers.
pub mod provider;
/// Agent session and turn-event runtime.
pub mod runtime;
/// Markdown skill loading and registry.
pub mod skills;
/// Session persistence backends.
pub mod storage;
/// Subagent definitions, registry, and fork execution.
pub mod subagent;
/// Persistent task tracking.
pub mod tasks;
/// Token-counting helpers.
pub mod tokens;
/// Tool trait, registry, validation, and execution context.
pub mod tool;
/// Built-in tools.
pub mod tools;

// Approval — asynchronous human-in-the-loop gating for tool calls.
pub use approval::{
    ApprovalDecision, ApprovalHandler, ApprovalRequest, AutoDenyHandler, FixedDecisionHandler,
};
// Compaction — history- and message-level shrinking strategies.
pub use compaction::{CompactionStrategy, SummaryCompaction};
// Configuration — the session config aggregate, task path, and token-budget knob.
pub use config::{AgentConfig, TaskPath, TokenBudget};
// Diagnostics — sanitized local recording of tool execution failures.
pub use diagnostics::{
    JsonlToolDiagnosticsSink, NoopToolDiagnosticsSink, SanitizedToolFailure, ToolDiagnosticsSink,
    ToolFailureEvent, ToolFailureKind, ToolFailureSanitizer,
};
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
pub use memory::{
    MemoryCategory, MemoryEntry, MemoryFormat, MemoryMaintenanceAction,
    MemoryMaintenanceActionKind, MemoryMaintenancePolicy, MemoryMaintenanceReport, MemoryQuery,
    MemorySort, MemoryStatus, MemoryStore, UpsertOutcome, unix_timestamp,
};
pub use tools::{
    MemoryEditTool, MemoryGrepTool, MemoryReadTool, MemoryStatusTool, MemoryWriteTool,
};
// Code index — lightweight repository search and path/line lookup.
pub use code_index::{CodeContextLine, CodeIndex, CodeSearchMatch, IndexedFile};
// Metrics — session-level counters accumulated by the runtime.
pub use config::CancellationState;
pub use metrics::SessionMetrics;
// Test helper — pre-canned [`ModelProvider`] for unit tests.
pub use mock::MockProvider;
// Permissions — rule-based gating of tool calls.
pub use permissions::{PermissionEngine, PermissionRule, RuleDecision};
// Provider — the trait downstream LLM backends implement, plus built-in impls.
pub use provider::{
    CompletionRequest, CompletionResponse, DeepSeekBalance, DeepSeekBalanceInfo,
    DeepSeekChatOptions, DeepSeekConfig, DeepSeekFimChoice, DeepSeekFimRequest,
    DeepSeekFimResponse, DeepSeekModel, DeepSeekModelList, DeepSeekProvider,
    DeepSeekResponseFormat, ErasedProvider, ModelHint, ModelProvider, ProviderEvent,
    RoutedModelConfig, RoutedProvider, StopReason, TokenUsage,
};
// Runtime — the agent session and the streaming turn loop.
pub use runtime::{
    AgentSession, TurnEvent, TurnInputReceiver, TurnInputSender, TurnResult, turn_input_channel,
};
// Skills — user-defined slash-commands loaded from markdown files.
pub use skills::{Skill, SkillArg, SkillLoader, SkillRegistry, SkillSource};
// Storage — persistence backends for saving and resuming sessions.
pub use storage::{JsonlStorage, NoopStorage, Storage};
// Subagent — nested agent run exposed as a tool and Fork concurrent-execution engine.
pub use subagent::{
    AgentDefinition, AgentIsolation, AgentSource, ForkExecution, ForkLens, ForkResult, ForkShared,
    SubagentRegistry, SubagentTool, Synapse, register_subagent_tool,
};
// Tasks — task management system with tracking, persistence, and tool integration.
pub use tasks::{Task, TaskManager, TaskStatus};
// MCP — stdio-based Model Context Protocol client + manager + bridge.
pub use mcp::{McpClient, McpManager, McpTool, McpToolBridge};
// Plugin — marketplace-based plugin system for extensibility.
pub use plugin::{BUILTIN_MARKETPLACE, PluginError, PluginId, PluginPromptSection};
// Prompt system — modular, cache-aware construction of the system prompt.
pub use prompt::{
    CwdSection, DateSection, GitStatusSection, IdentitySection, McpSection, MemorySection,
    ProfileSection, PromptAssembly, PromptSection, PromptStability, SafetySection,
    ShellAwareToolUsageSection, SkillsSection, TaskGuidanceSection, ToneStyleSection,
    ToolUsageSection, ToolsSection,
};
// Tool abstraction — the trait every callable capability implements, plus its registry.
pub use tool::validate::{ValidationError, ValidationResult, validate_arguments};
pub use tool::{
    InterruptBehavior, PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput,
    ToolProgress, ToolRegistry,
};
// Built-in tools — filesystem, shell, search, web, user interaction.
pub use tools::{
    AskUserQuestionTool, BrowserBackTool, BrowserClickTool, BrowserCloseTool, BrowserFindUrlTool,
    BrowserManager, BrowserNavigateTool, BrowserScreenshotTool, BrowserScrollTool,
    BrowserSelectTool, BrowserStartTool, BrowserStateTool, BrowserTypeTool, CodeContextTool,
    CodeIndexRefreshTool, CodeSearchTool, DefaultShell, FileEditTool, FileReadTool, FileWriteTool,
    GlobTool, GrepTool, PowerShellTool, ShellTool, SkillTool, TaskCreateTool, TaskGetTool,
    TaskListTool, TaskOutputTool, TaskStopTool, TaskUpdateTool, WebFetchTool, WebSearchTool,
    register_core_tools, register_core_tools_with_shell, register_memory_tools,
    register_task_tools,
};
