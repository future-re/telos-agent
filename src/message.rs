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

impl Message {
    /// Build a system message containing a single text block.
    pub fn system(text: impl Into<String>) -> Self {
        Self::text(Role::System, text)
    }

    /// Build a user message containing a single text block.
    pub fn user(text: impl Into<String>) -> Self {
        Self::text(Role::User, text)
    }

    /// Build an assistant message containing a single text block.
    ///
    /// For an assistant message that includes tool calls, construct the
    /// [`Message`] directly with the appropriate [`ContentBlock`]s.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::text(Role::Assistant, text)
    }

    /// Build a tool message wrapping a single [`ToolResult`].
    pub fn tool(result: ToolResult) -> Self {
        Self {
            role: Role::Tool,
            blocks: vec![ContentBlock::ToolResult(result)],
        }
    }

    /// Build a tool message wrapping multiple [`ToolResult`]s.
    ///
    /// Used after a single assistant message issued several tool calls — every
    /// result is bundled into one tool-role message so the model sees them
    /// together on the next iteration.
    pub fn tool_results(results: Vec<ToolResult>) -> Self {
        Self {
            role: Role::Tool,
            blocks: results.into_iter().map(ContentBlock::ToolResult).collect(),
        }
    }

    /// Build a message with `role` and a single [`TextBlock`] payload.
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            blocks: vec![ContentBlock::Text(TextBlock { text: text.into() })],
        }
    }

    /// Concatenate all [`TextBlock`] contents in this message, separated by newlines.
    ///
    /// Non-text blocks (tool calls, tool results) are skipped.
    pub fn text_content(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Iterate over [`ToolCall`]s in this message (typically used on assistant messages).
    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.blocks.iter().filter_map(|block| match block {
            ContentBlock::ToolCall(call) => Some(call),
            _ => None,
        })
    }

    /// Iterate over [`ToolResult`]s in this message (typically used on tool-role messages).
    pub fn tool_results_iter(&self) -> impl Iterator<Item = &ToolResult> {
        self.blocks.iter().filter_map(|block| match block {
            ContentBlock::ToolResult(result) => Some(result),
            _ => None,
        })
    }
}

/// A single piece of content within a [`Message`].
///
/// Messages are made of heterogeneous blocks because modern LLMs interleave
/// natural language, tool invocations, and tool outputs within a single turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContentBlock {
    /// Natural-language text.
    Text(TextBlock),
    /// A request from the assistant to invoke a tool.
    ToolCall(ToolCall),
    /// The result of having invoked a tool.
    ToolResult(ToolResult),
}

/// A plain-text block of content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
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
