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
    pub fn system(text: impl Into<String>) -> Self {
        Self::text(Role::System, text)
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::text(Role::User, text)
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self::text(Role::Assistant, text)
    }

    pub fn tool(result: ToolResult) -> Self {
        Self { role: Role::Tool, blocks: vec![ContentBlock::ToolResult(result)] }
    }

    pub fn tool_results(results: Vec<ToolResult>) -> Self {
        Self {
            role: Role::Tool,
            blocks: results.into_iter().map(ContentBlock::ToolResult).collect(),
        }
    }

    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self { role, blocks: vec![ContentBlock::text(text)] }
    }

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

    pub fn thinking_content(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Thinking(thinking) => Some(thinking.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.blocks.iter().filter_map(|block| match block {
            ContentBlock::ToolCall(call) => Some(call),
            _ => None,
        })
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemReminder {
    PlanMode,
    Compaction { reason: String },
    ProviderContext,
    HookInterception { phase: String, name: String },
    ToolResult { tool_name: String, note: String },
    MemoryInjection { content: String },
    SkillDiscovery { content: String },
}

impl SystemReminder {
    pub fn render(&self) -> String {
        let body = match self {
            Self::PlanMode => "You are entering plan mode. Follow the plan instructions and do not write implementation code until the plan is approved.".to_string(),
            Self::Compaction { reason } => format!(
                "Prior messages were compacted (reason: {reason}). Some context may have been summarized."
            ),
            Self::ProviderContext => "The provider/model context has changed. Adjust to any new instructions or constraints.".to_string(),
            Self::HookInterception { phase, name } => format!(
                "A hook intercepted this turn during the {phase} phase ({name}). Treat hook output as user feedback."
            ),
            Self::ToolResult { tool_name, note } => format!("Tool `{tool_name}` reported: {note}"),
            Self::MemoryInjection { content } | Self::SkillDiscovery { content } => content.clone(),
        };
        format!("<system-reminder>\n{body}\n</system-reminder>")
    }
}

impl ContentBlock {
    /// Create a new text block.
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text(TextBlock { text: text.into() })
    }

    /// Create a new tool call block.
    pub fn tool_call(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        ContentBlock::ToolCall(ToolCall { id: id.into(), name: name.into(), arguments })
    }

    /// Create a new tool result block.
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: Value,
        is_error: bool,
    ) -> Self {
        ContentBlock::ToolResult(ToolResult {
            tool_call_id: tool_call_id.into(),
            name: name.into(),
            content,
            is_error,
        })
    }

    /// Create a new thinking block.
    pub fn thinking(
        text: impl Into<String>,
        signature: Option<impl Into<String>>,
        is_redacted: bool,
    ) -> Self {
        ContentBlock::Thinking(ThinkingBlock {
            text: text.into(),
            signature: signature.map(|s| s.into()),
            is_redacted,
        })
    }
}
