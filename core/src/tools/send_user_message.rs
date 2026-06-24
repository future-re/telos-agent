//! `SendUserMessage` tool (the "BriefTool" equivalent).
//!
//! The primary output channel for the agent to send messages to the user.
//! Supports markdown-formatted messages with optional file attachments
//! (images, diffs, logs). Has `normal` and `proactive` status labels.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

/// `SendUserMessage` — send a message (with optional attachments) to the user.
pub struct SendUserMessageTool;

impl SendUserMessageTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SendUserMessageTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SendUserMessageTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "SendUserMessage".into(),
            description:
                "Primary output channel. Sends a markdown message to the user with optional file attachments (images, diffs, logs)."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The message for the user. Supports markdown formatting."
                    },
                    "attachments": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional file paths (absolute or relative to cwd) to attach. Use for photos, screenshots, diffs, logs, or any file the user should see."
                    },
                    "status": {
                        "type": "string",
                        "enum": ["normal", "proactive"],
                        "description": "'proactive' when surfacing something the user hasn't asked for (task completion while away, blocker, unsolicited status update). 'normal' when replying to the user."
                    }
                },
                "required": ["message", "status"]
            }),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(SEND_USER_MESSAGE_PROMPT)
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["Brief", "send_user_message"]
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let message = arguments
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::Validation("missing `message` field".into()))?;

        let status = arguments.get("status").and_then(|v| v.as_str()).unwrap_or("normal");

        // Resolve attachments
        let attachment_paths: Vec<String> = arguments
            .get("attachments")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let mut resolved: Vec<serde_json::Value> = Vec::new();
        for raw_path in &attachment_paths {
            let path = resolve_path(&context.cwd, raw_path);
            match std::fs::metadata(&path) {
                Ok(meta) if meta.is_file() => {
                    let is_image = is_image_path(&path);
                    resolved.push(json!({
                        "path": raw_path,
                        "size": meta.len(),
                        "is_image": is_image
                    }));
                }
                Ok(_) => {
                    resolved.push(json!({
                        "path": raw_path,
                        "error": "not a regular file"
                    }));
                }
                Err(e) => {
                    resolved.push(json!({
                        "path": raw_path,
                        "error": format!("cannot access: {e}")
                    }));
                }
            }
        }

        let attachment_count = resolved.len();

        Ok(ToolOutput::json(json!({
            "message": message,
            "attachments": resolved,
            "status": status,
            "sent_at": chrono_now(),
            "feedback": format!("Message delivered to user{}",
                if attachment_count > 0 {
                    format!(" with {attachment_count} attachment(s)")
                } else {
                    String::new()
                }
            )
        })))
    }
}

fn resolve_path(cwd: &Path, raw: &str) -> PathBuf {
    let p = PathBuf::from(raw);
    if p.is_absolute() { p } else { cwd.join(p) }
}

fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp"
            )
        })
        .unwrap_or(false)
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without pulling in the `chrono` crate.
    use std::time::SystemTime;
    let now = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    // Approximate: not a full calendar conversion but good enough for a timestamp.
    format!("{secs}")
}

const SEND_USER_MESSAGE_PROMPT: &str = r#"SendUserMessage is your primary output channel.

Use this for all replies to the user. Pattern: acknowledge what you understood → do the work → SendUserMessage with the result. For long work, send checkpoint messages.

- `status`: "normal" when replying, "proactive" for unsolicited updates (task completed while user away, blocker hit)
- `message`: markdown. Lead with the answer, be concise, reference file paths.
- `attachments` (optional): paths for images, diffs, logs."#;
