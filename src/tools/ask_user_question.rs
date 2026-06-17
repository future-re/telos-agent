use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{InterruptBehavior, Tool, ToolContext, ToolDefinition, ToolOutput};

/// Tool that presents structured questions to the user.
///
/// This tool returns a `questions_ready` signal with the questions for the host
/// application to present. The host is responsible for collecting answers and
/// feeding them back in the next turn.
pub struct AskUserQuestionTool;

fn make_schema() -> Value {
    serde_json::from_str(
        r#"{
        "type": "object",
        "properties": {
            "questions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "The question text to display"
                        },
                        "header": {
                            "type": "string",
                            "description": "Section header for the question"
                        },
                        "options": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "label": {
                                        "type": "string",
                                        "description": "Short label for the option"
                                    },
                                    "description": {
                                        "type": "string",
                                        "description": "Detailed description of the option"
                                    }
                                },
                                "required": ["label", "description"]
                            },
                            "description": "Available answer choices"
                        },
                        "multiSelect": {
                            "type": "boolean",
                            "default": false,
                            "description": "Allow selecting multiple options"
                        }
                    },
                    "required": ["question", "header", "options"]
                }
            }
        },
        "required": ["questions"]
    }"#,
    )
    .expect("static schema is valid JSON")
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "AskUserQuestion".into(),
            description: "Ask the user one or more questions with optional selections. Returns structured questions for the host to present.".into(),
            input_schema: make_schema(),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["ask_user"]
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Block
    }

    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let questions = args
            .get("questions")
            .and_then(|v| v.as_array())
            .ok_or_else(|| AgentError::Validation("missing questions array".into()))?;

        if questions.is_empty() {
            return Err(AgentError::Validation("questions array is empty".into()));
        }

        // Validate each question has required fields
        for (i, q) in questions.iter().enumerate() {
            if q.get("question").and_then(|v| v.as_str()).is_none_or(|s| s.is_empty()) {
                return Err(AgentError::Validation(format!(
                    "question at index {i} is missing a non-empty `question` field"
                )));
            }
            if q.get("header").and_then(|v| v.as_str()).is_none_or(|s| s.is_empty()) {
                return Err(AgentError::Validation(format!(
                    "question at index {i} is missing a non-empty `header` field"
                )));
            }
            let options = q.get("options").and_then(|v| v.as_array());
            match options {
                Some(opts) if opts.is_empty() => {
                    return Err(AgentError::Validation(format!(
                        "question at index {i} has an empty `options` array"
                    )));
                }
                None => {
                    return Err(AgentError::Validation(format!(
                        "question at index {i} is missing `options` array"
                    )));
                }
                Some(_) => {}
            }
        }

        Ok(ToolOutput::json(json!({
            "status": "questions_ready",
            "questions": questions,
            "instruction": "Please answer these questions to continue."
        })))
    }
}
