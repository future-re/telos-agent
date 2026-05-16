use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

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
        Self {
            role: Role::Tool,
            blocks: vec![ContentBlock::ToolResult(result)],
        }
    }

    pub fn tool_results(results: Vec<ToolResult>) -> Self {
        Self {
            role: Role::Tool,
            blocks: results.into_iter().map(ContentBlock::ToolResult).collect(),
        }
    }

    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            blocks: vec![ContentBlock::Text(TextBlock { text: text.into() })],
        }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContentBlock {
    Text(TextBlock),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub content: Value,
    pub is_error: bool,
}
