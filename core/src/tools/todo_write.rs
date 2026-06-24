//! `TodoWrite` tool — lightweight in-session progress tracking.
//!
//! Replaces the entire task list atomically (replace-all semantics). Items have
//! no IDs — the model keeps the full desired list. Three statuses: `pending`,
//! `in_progress`, `completed`. Ephemeral — not persisted across sessions.
//!
//! Differs from the Task system (persistent, per-task CRUD with IDs and
//! dependency tracking). This is a *session progress tracker* — simple,
//! instantaneous, and heavily guided by the prompt.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

/// In-memory store for the current todo list.
#[derive(Debug, Clone, Default)]
pub struct TodoList {
    pub items: Vec<TodoItem>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub active_form: String,
}

pub type SharedTodoList = Arc<Mutex<TodoList>>;

/// `TodoWrite` — replace the todo list with the given items.
pub struct TodoWriteTool {
    todos: SharedTodoList,
}

impl TodoWriteTool {
    pub fn new(todos: SharedTodoList) -> Self {
        Self { todos }
    }
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "TodoWrite".into(),
            description: "Use this tool to create and manage a structured task list for your current coding session. \
This helps you track progress, organize complex tasks, and demonstrate thoroughness to the user. \
It also helps the user understand the progress of the task and overall progress of their requests.\n\n\
**When to Use**\n\
- Complex multi-step tasks (3+ distinct steps)\n\
- Non-trivial tasks requiring careful planning\n\
- User explicitly requests todo list\n\
- User provides multiple tasks (numbered/comma-separated)\n\
- After receiving new instructions — capture requirements as todos (merge with existing)\n\n\
**When NOT to Use**\n\
- Single, straightforward tasks\n\
- Trivial tasks with no organizational benefit\n\
- Tasks completable in < 3 trivial steps\n\
- Purely conversational/informational requests\n\
- Todo items should NOT include operational actions from other tools\n\n\
**Task Management Rules**\n\
- Exactly ONE item can be `in_progress` at a time — finish or pause others first.\n\
- Update status in real-time — mark complete right after finishing, not in batches.\n\
- When claiming new work: mark it `in_progress` BEFORE starting.\n\
- Don't keep stale items — remove tasks that are no longer relevant.\n\
- Always provide `active_form` (present continuous) alongside `content` (imperative).\n\
- Before completing the turn, verify all todos are accurate.\n\
- Keep the list focused — only the current task has details.\n\n\
**Completion Criteria**\n\
Only mark as `completed` when FULLY accomplished:\n\
- No failing tests\n\
- No partial implementation\n\
- No unresolved errors\n\
- All acceptance criteria met\n\
- If verification failed, keep as `in_progress` or add a new verification todo.\n\n\
**Batch Todo Completes**\n\
When finishing the final task before handing off to the user, you may batch-update \
all remaining items to `completed` in one call."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "description": "The complete todo list (replaces all existing items)",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": {
                                    "type": "string",
                                    "description": "The task content (imperative form, e.g. 'Implement login handler')"
                                },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"],
                                    "description": "Current status of the task"
                                },
                                "active_form": {
                                    "type": "string",
                                    "description": "Present continuous form (e.g. 'Implementing login handler')"
                                }
                            },
                            "required": ["content", "status", "active_form"]
                        }
                    }
                },
                "required": ["todos"]
            }),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(TODO_WRITE_PROMPT)
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["todo_write", "todowrite"]
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let new_items: Vec<TodoItem> = {
            let arr = arguments
                .get("todos")
                .and_then(|v| v.as_array())
                .ok_or_else(|| AgentError::Validation("missing `todos` array".into()))?;

            arr.iter()
                .map(|item| TodoItem {
                    content: item.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    status: item
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pending")
                        .to_string(),
                    active_form: item
                        .get("active_form")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        };

        let old_items = {
            let mut todos = self.todos.lock().unwrap();
            std::mem::replace(&mut todos.items, new_items).clone()
        };

        let new_items = self.todos.lock().unwrap().items.clone();

        // Auto-clear: if all items are completed, reset to empty
        if !new_items.is_empty() && new_items.iter().all(|i| i.status == "completed") {
            self.todos.lock().unwrap().items.clear();
        }

        // Build verification nudge if applicable
        let mut extra = Vec::new();
        let completed_count = new_items.iter().filter(|i| i.status == "completed").count();
        if completed_count >= 3 {
            let has_verify_mention =
                new_items.iter().any(|i| i.content.to_lowercase().contains("verif"));
            if !has_verify_mention {
                extra.push("Consider spawning a Verify subagent to validate the completed work.");
            }
        }

        let mut result = json!({
            "old_todos": old_items,
            "new_todos": new_items,
            "updated": true,
            "message": format!("Todo list updated: {} items", new_items.len())
        });

        if let Some(obj) = result.as_object_mut()
            && !extra.is_empty()
        {
            obj.insert("hints".into(), json!(extra));
        }

        Ok(ToolOutput::json(result))
    }
}

const TODO_WRITE_PROMPT: &str = r#"TodoWrite manages your session task list.

**Rules:**
1. Exactly ONE item `in_progress` at a time.
2. Update status immediately after finishing — mark new work `in_progress` before starting.
3. Always include `active_form` (present continuous) for each item.
4. Only mark `completed` when fully done (no failing tests, no partial work).
5. Remove stale items promptly. Merge new user instructions into the full list."#;
