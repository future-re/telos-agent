pub mod compaction;
pub mod error;
pub mod hooks;
pub mod message;
pub mod mock;
pub mod provider;
pub mod runtime;
pub mod tool;

pub use error::AgentError;
pub use hooks::{Hook, HookContext, HookPhase, HookRegistry};
pub use message::{ContentBlock, Message, Role, TextBlock, ToolCall, ToolResult};
pub use mock::MockProvider;
pub use provider::{
    AnthropicConfig, AnthropicProvider, CompletionRequest, CompletionResponse, ModelProvider,
    OpenAIConfig, OpenAIProvider, StopReason,
};
pub use runtime::{AgentConfig, AgentSession, TurnEvent, TurnResult};
pub use tool::{
    InterruptBehavior, PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput,
    ToolRegistry,
};
