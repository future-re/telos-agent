//! Message types used in conversations with the model and between tools.
//!
//! A conversation is an ordered list of [`Message`]s. Each message has a [`Role`]
//! (system / user / assistant / tool) and one or more [`ContentBlock`]s. Blocks
//! carry the actual payload — plain text, a tool call requested by the model,
//! or the result of executing a tool call.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Speaker of a message in the conversation.
///
/// Providers may map these to their own role taxonomies (e.g. OpenAI-compatible
/// APIs render `Tool` results as separate `tool` messages — see provider implementations).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Instructions/persona supplied before the conversation starts.
    System,
    /// Input authored by the human user.
    User,
    /// Output produced by the model.
    Assistant,
    /// Result of executing a tool call previously requested by the assistant.
    Tool,
}

/// A single message in the conversation: a role plus an ordered list of content blocks.
///
/// A message can be heterogeneous — an assistant message often contains a
/// [`TextBlock`] explaining its reasoning followed by one or more [`ToolCall`]s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub blocks: Vec<ContentBlock>,
}

/// A single piece of content within a [`Message`].
///
/// Messages are made of heterogeneous blocks because modern LLMs interleave
/// natural language, tool invocations, tool outputs, and reasoning traces
/// within a single turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContentBlock {
    /// Natural-language text.
    Text(TextBlock),
    /// A request from the assistant to invoke a tool.
    ToolCall(ToolCall),
    /// The result of having invoked a tool.
    ToolResult(ToolResult),
    /// A reasoning trace emitted by a thinking-capable model.
    Thinking(ThinkingBlock),
}

/// A plain-text block of content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
}

/// A reasoning trace produced by a thinking-capable model.
///
/// Kept separate from [`TextBlock`] so consumers can choose whether to surface
/// the reasoning to end users.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingBlock {
    /// The raw reasoning text.
    pub text: String,
    /// Cryptographic signature provided by some providers (e.g. Claude 3.7
    /// thinking) to verify the reasoning block was not tampered with.
    pub signature: Option<String>,
    /// Whether the reasoning content was redacted by the provider.
    pub is_redacted: bool,
}

/// A request from the assistant to invoke a named tool with structured arguments.
///
/// `id` is the provider-assigned identifier used to correlate a call with its
/// [`ToolResult`] on the next turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// The outcome of executing a [`ToolCall`].
///
/// `tool_call_id` must match the originating call's `id`. `content` is the JSON
/// payload returned to the model; `is_error` flags execution failures so the
/// model can recover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub content: Value,
    pub is_error: bool,
}