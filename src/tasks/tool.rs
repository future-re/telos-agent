use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::error::AgentError;
use crate::tasks::{Task, TaskManager, TaskStatus};
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

// ── TaskCreate ──────────────────────────────────────────
pub struct TaskCreateTool {
    manager: Arc<TaskManager>,
}
impl TaskCreateTool {
    pub fn new(m: Arc<TaskManager>) -> Self {
        Self { manager: m }
    }
}
#[async_trait]
impl Tool for TaskCreateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "TaskCreate".into(),
            description: "Create a new task for tracking work.".into(),
            input_schema: json!({"type":"object","properties":{"subject":{"type":"string"},"description":{"type":"string"},"blocked_by":{"type":"array","items":{"type":"string"},"default":[]}},"required":["subject","description"]}),
        }
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let subject = args.get("subject").and_then(|v| v.as_str()).unwrap_or("");
        let desc = args.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let blocked_by: Vec<String> = args
            .get("blocked_by")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let id = uuid_v4();
        let task = Task {
            id: id.clone(),
            subject: subject.into(),
            description: desc.into(),
            status: TaskStatus::Pending,
            blocked_by,
            blocks: vec![],
            output: None,
        };
        self.manager.create(task);
        Ok(ToolOutput::json(json!({"task_id": id, "status": "created"})))
    }
}

fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("task_{:x}", now.as_nanos())
}

// ── TaskGet ─────────────────────────────────────────────
pub struct TaskGetTool {
    manager: Arc<TaskManager>,
}
impl TaskGetTool {
    pub fn new(m: Arc<TaskManager>) -> Self {
        Self { manager: m }
    }
}
#[async_trait]
impl Tool for TaskGetTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "TaskGet".into(),
            description: "Get details of a task by ID.".into(),
            input_schema: json!({"type":"object","properties":{"task_id":{"type":"string"}},"required":["task_id"]}),
        }
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        match self.manager.get(id) {
            Some(task) => Ok(ToolOutput::json(serde_json::to_value(task).unwrap_or_default())),
            None => Ok(ToolOutput::json(json!({"error": "task not found"}))),
        }
    }
}

// ── TaskList ────────────────────────────────────────────
pub struct TaskListTool {
    manager: Arc<TaskManager>,
}
impl TaskListTool {
    pub fn new(m: Arc<TaskManager>) -> Self {
        Self { manager: m }
    }
}
#[async_trait]
impl Tool for TaskListTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "TaskList".into(),
            description: "List all tasks with status.".into(),
            input_schema: json!({"type":"object","properties":{}}),
        }
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn invoke(&self, _: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let tasks = self.manager.list();
        Ok(ToolOutput::json(serde_json::to_value(tasks).unwrap_or_default()))
    }
}

// ── TaskUpdate ──────────────────────────────────────────
pub struct TaskUpdateTool {
    manager: Arc<TaskManager>,
}
impl TaskUpdateTool {
    pub fn new(m: Arc<TaskManager>) -> Self {
        Self { manager: m }
    }
}
#[async_trait]
impl Tool for TaskUpdateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "TaskUpdate".into(),
            description: "Update task status: pending, in_progress, completed, deleted.".into(),
            input_schema: json!({"type":"object","properties":{"task_id":{"type":"string"},"status":{"type":"string","enum":["pending","in_progress","completed","deleted"]}},"required":["task_id","status"]}),
        }
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        let status_str = args.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
        let status = match status_str {
            "pending" => TaskStatus::Pending,
            "in_progress" => TaskStatus::InProgress,
            "completed" => TaskStatus::Completed,
            "deleted" => TaskStatus::Deleted,
            _ => return Err(AgentError::Validation("invalid status".into())),
        };
        self.manager.update(id, status);
        Ok(ToolOutput::json(json!({"task_id": id, "status": status_str, "updated": true})))
    }
}
