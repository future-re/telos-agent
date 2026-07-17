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
//! - [`AgentRuntime`] — owns the provider, tools, and runtime configuration.
//! - [`AgentSession`] — concurrency-safe conversation state created by the runtime.
//! - [`AgentConfig`] — configures cwd, env, storage, approvals, compaction,
//!   token budgets, policies, and cancellation.
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
//! [`ModelProvider`], then drive turns through [`AgentRuntime::run_turn`] or
//! [`AgentRuntime::start_turn`] for a UI-friendly event stream.
//!
//! ```rust
//! use telos_agent::{
//!     AgentConfig, AgentError, AgentRuntime, CompletionResponse, Message,
//!     MockProvider, StopReason, ToolRegistry,
//! };
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), AgentError> {
//!     let provider = Arc::new(MockProvider::new(vec![CompletionResponse {
//!         message: Message::assistant("done"),
//!         stop_reason: StopReason::EndTurn,
//!         usage: None,
//!         model: None,
//!     }]));
//!
//!     let tools = ToolRegistry::new();
//!     let runtime = AgentRuntime::new(AgentConfig {
//!         base_system_prompt: Some("You are concise.".into()),
//!         ..Default::default()
//!     }, provider, tools)?;
//!     let session = runtime.create_session().await?;
//!
//!     let result = runtime.run_turn(&session, "hello").await?;
//!     assert_eq!(result.final_message.text_content(), "done");
//!     Ok(())
//! }
//! ```
//!
//! # Streaming Turns
//!
//! Hosts that render live UI should use [`AgentRuntime::start_turn`]. Its
//! [`TurnHandle`] yields [`TurnEvent`] values in order, so the host can update assistant text,
//! thinking text, tool activity, approval prompts, token usage, and completion
//! state without parsing model messages directly.
//!
//! # Extension Points
//!
//! - Implement [`ModelProvider`] to add a new model backend.
//! - Implement [`Tool`] to expose a new capability.
//! - Implement [`ApprovalHandler`] for host-specific approval UI.
//! - Implement [`Policy`] to enforce semantic session, model, turn, or tool rules.
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
//! | [`agent`] | Runtime, sessions, turns, context, prompts, policies, and compaction. |
//! | [`model`] | Provider APIs, model messages, token counting, and test doubles. |
//! | [`tools`] | Tool APIs, execution, built-ins, approvals, permissions, and command safety. |
//! | [`knowledge`] | Memory, skills, tasks, and repository indexing. |
//! | [`integrations`] | MCP, plugins, and external event channels. |
//! | [`orchestration`] | Subagents and multi-agent teams. |
//! | [`diagnostics`] / [`metrics`] | Sanitized tool failure records and session counters. |

/// Agent lifecycle, context, prompting, policies, and turn execution.
pub mod agent;
/// Runtime configuration and cancellation state.
pub mod config;
/// Sanitized tool failure diagnostics.
pub mod diagnostics;
/// Error types shared across runtime, tools, and providers.
pub mod error;
/// External event, protocol, and plugin integrations.
pub mod integrations;
/// Persistent knowledge, skills, tasks, and repository indexing.
pub mod knowledge;
/// Runtime metrics accumulated by sessions.
pub mod metrics;
/// Model messages, providers, token accounting, and test doubles.
pub mod model;
/// Nested-agent and multi-agent orchestration.
pub mod orchestration;
/// Session persistence backends.
pub mod storage;

/// Tool APIs, execution, built-ins, approvals, permissions, and command safety.
pub mod tools;

// Approval — asynchronous human-in-the-loop gating for tool calls.
pub use tools::approval::{
    ApprovalDecision, ApprovalHandler, ApprovalRequest, AutoDenyHandler, FixedDecisionHandler,
};
// Compaction — history- and message-level shrinking strategies.
pub use agent::compaction::{HistoryCompactionStrategy, SummaryHistoryCompaction};
// Configuration — the session config aggregate, task path, and token-budget knob.
pub use config::{AgentConfig, TaskPath, TokenBudget, platform_base_env};
// Diagnostics — sanitized local recording of tool execution failures.
pub use diagnostics::{
    JsonlToolDiagnosticsSink, NoopToolDiagnosticsSink, SanitizedToolFailure, ToolDiagnosticsSink,
    ToolFailureEvent, ToolFailureKind, ToolFailureSanitizer,
};
// Errors — the single failure type used across the crate.
pub use error::{AgentError, ProviderError};
// Event channel — bidirectional HTTP event channel for external pub/sub.
pub use integrations::event_channel::{
    EventChannel, EventChannelConfig, ExternalEvent, Subscription,
};
// Tool executor — direct entry points for callers that bypass the turn loop.
pub use tools::executor::{
    ToolExecutionEvent, ToolExecutionOutput, ToolExecutionStreamItem, execute_tool_calls_stream,
};
// Semantic session, model, turn, and tool policies.
pub use agent::policies::{
    Policy, PolicyContext, PolicyDecision, PolicyEntry, PolicyOutcome, PolicyPoint, PolicyRegistry,
    SessionMode,
};
// Message model — the lingua franca between session, provider, and tools.
pub use model::message::{
    ContentBlock, Message, Role, TextBlock, ThinkingBlock, ToolCall, ToolResult,
};
// Memory — persistent cross-session agent memory.
pub use knowledge::memory::ProfileManager;
pub use knowledge::memory::{
    MemoryCategory, MemoryEntry, MemoryFormat, MemoryMaintenanceAction,
    MemoryMaintenanceActionKind, MemoryMaintenancePolicy, MemoryMaintenanceReport, MemoryQuery,
    MemorySort, MemoryStatus, MemoryStore, UpsertOutcome, unix_timestamp,
};
pub use tools::builtin::{
    MemoryEditTool, MemoryGrepTool, MemoryReadTool, MemoryStatusTool, MemoryWriteTool,
};
// Code index — lightweight repository search and path/line lookup.
pub use knowledge::code_index::{CodeContextLine, CodeIndex, CodeSearchMatch, IndexedFile};
// Metrics — session-level counters accumulated by the runtime.
pub use config::CancellationState;
pub use metrics::SessionMetrics;
// Test helper — pre-canned [`ModelProvider`] for unit tests.
pub use model::mock::MockProvider;
// Permissions — rule-based gating of tool calls.
pub use tools::permissions::{PermissionEngine, PermissionRule, RuleDecision};
// Provider — the trait downstream LLM backends implement, plus built-in impls.
pub use model::provider::{
    CompletionRequest, CompletionResponse, DeepSeekBalance, DeepSeekBalanceInfo,
    DeepSeekChatOptions, DeepSeekConfig, DeepSeekFimChoice, DeepSeekFimRequest,
    DeepSeekFimResponse, DeepSeekModel, DeepSeekModelList, DeepSeekProvider,
    DeepSeekResponseFormat, ErasedProvider, ModelHint, ModelProvider, ProviderEvent,
    RoutedModelConfig, RoutedProvider, StopReason, TokenUsage,
};

pub use agent::runtime::{AgentRuntime, AgentSession, TurnHandle};
// Context injection services.
pub use agent::context::{MemoryInjector, SkillInjector};
// Turn — streaming event types and input channel.
pub use agent::turn::{
    TurnEvent, TurnInputReceiver, TurnInputSender, TurnResult, turn_input_channel,
};
// Skills — user-defined slash-commands loaded from markdown files.
pub use knowledge::skills::{Skill, SkillArg, SkillLoader, SkillRegistry, SkillSource};
// Storage — persistence backends for saving and resuming sessions.
pub use storage::{JsonlStorage, NoopStorage, Storage};
// Subagent — nested agent run exposed as a tool and Fork concurrent-execution engine.
pub use orchestration::subagent::{
    AgentDefinition, AgentIsolation, AgentSource, ForkExecution, ForkLens, ForkResult, ForkShared,
    SubagentRegistry, SubagentTool, Synapse, register_subagent_tool,
};
// Tasks — task management system with tracking, persistence, and tool integration.
pub use knowledge::tasks::{Task, TaskManager, TaskStatus};
pub use orchestration::team::{
    TeamConfig, TeamMember, cleanup_team, has_active_members, lead_agent_id, load_team_config,
    save_team_config, team_config_path, team_tasks_dir, teams_root,
};

// MCP — stdio-based Model Context Protocol client + manager + bridge.
pub use integrations::mcp::{McpClient, McpManager, McpTool, McpToolBridge};
// Plugin — marketplace-based plugin system for extensibility.
pub use integrations::plugin::{
    BUILTIN_MARKETPLACE, PluginError, PluginId, PluginPromptSection, PluginRegistry,
};
// Prompt system — modular, cache-aware construction of the system prompt.
pub use agent::prompt::{
    CwdSection, DateSection, GitStatusSection, IdentitySection, McpSection, MemorySection,
    ProfileSection, PromptAssembly, PromptProfile, PromptSection, PromptSectionStat,
    PromptStability, PromptStats, SafetySection, ShellAwareToolUsageSection, SkillsSection,
    TaskGuidanceSection, ToneStyleSection, ToolUsageSection, ToolsSection,
};

// Tool abstraction — the trait every callable capability implements, plus its registry.
pub use tools::api::validate::{ValidationError, ValidationResult, validate_arguments};
pub use tools::api::{
    InterruptBehavior, PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput,
    ToolProgress, ToolRegistry,
};
// Built-in tools — filesystem, shell, search, web, user interaction.
pub use tools::builtin::{
    AskUserQuestionTool, BrowserBackTool, BrowserClickTool, BrowserCloseTool, BrowserFindUrlTool,
    BrowserManager, BrowserNavigateTool, BrowserScreenshotTool, BrowserScrollTool,
    BrowserSelectTool, BrowserStartTool, BrowserStateTool, BrowserTypeTool, CodeContextTool,
    CodeIndexRefreshTool, CodeSearchTool, DefaultShell, EnterPlanModeTool, ExitPlanModeTool,
    FileEditTool, FileReadTool, FileWriteTool, GlobTool, GrepTool, PowerShellTool,
    SendUserMessageTool, ShellTool, SkillTool, TaskCreateTool, TaskGetTool, TaskListTool,
    TaskOutputTool, TaskStopTool, TaskUpdateTool, TeamCreateTool, TeamDeleteTool, TodoWriteTool,
    WebFetchTool, WebSearchTool, register_core_tools, register_core_tools_with_shell,
    register_memory_tools, register_task_tools,
};
