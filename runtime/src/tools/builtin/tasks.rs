use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::error::AgentError;
use crate::knowledge::tasks::{Task, TaskManager, TaskStatus};
use crate::tools::api::{Tool, ToolContext, ToolDefinition, ToolOutput};

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
            kind: None,
            agent_id: None,
            agent_type: None,
            worktree_path: None,
            error: None,
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

fn status_str(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
        TaskStatus::Deleted => "deleted",
    }
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

// ── TaskOutput ──────────────────────────────────────────
pub struct TaskOutputTool {
    manager: Arc<TaskManager>,
}
impl TaskOutputTool {
    pub fn new(m: Arc<TaskManager>) -> Self {
        Self { manager: m }
    }
}
#[async_trait]
impl Tool for TaskOutputTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "task_output".into(),
            description: "Read the current status and output for a background task.".into(),
            input_schema: json!({"type":"object","properties":{"task_id":{"type":"string"}},"required":["task_id"]}),
        }
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        let Some(task) = self.manager.get(id) else {
            return Ok(ToolOutput::json(json!({"task_id": id, "error": "task not found"})));
        };
        Ok(ToolOutput::json(json!({
            "task_id": task.id,
            "subject": task.subject,
            "description": task.description,
            "status": status_str(&task.status),
            "output": task.output,
            "kind": task.kind,
            "agent_id": task.agent_id,
            "agent_type": task.agent_type,
            "worktree_path": task.worktree_path,
            "error": task.error,
        })))
    }
}

// ── TaskStop ────────────────────────────────────────────
pub struct TaskStopTool {
    manager: Arc<TaskManager>,
}
impl TaskStopTool {
    pub fn new(m: Arc<TaskManager>) -> Self {
        Self { manager: m }
    }
}
#[async_trait]
impl Tool for TaskStopTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "task_stop".into(),
            description: "Request cancellation for a running background task.".into(),
            input_schema: json!({"type":"object","properties":{"task_id":{"type":"string"}},"required":["task_id"]}),
        }
    }
    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        let Some(task) = self.manager.get(id) else {
            return Ok(ToolOutput::json(json!({"task_id": id, "error": "task not found"})));
        };
        match task.status {
            TaskStatus::Completed
            | TaskStatus::Failed
            | TaskStatus::Cancelled
            | TaskStatus::Deleted => Ok(ToolOutput::json(json!({
                "task_id": id,
                "status": status_str(&task.status),
                "stopped": false
            }))),
            TaskStatus::Pending | TaskStatus::InProgress => {
                let cancellation_requested = self.manager.request_cancel(id);
                self.manager.cancel(id, "stopped by user".into());
                Ok(ToolOutput::json(json!({
                    "task_id": id,
                    "status": "cancelled",
                    "stopped": true,
                    "cancellation_requested": cancellation_requested
                })))
            }
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
            "failed" => TaskStatus::Failed,
            "cancelled" => TaskStatus::Cancelled,
            "deleted" => TaskStatus::Deleted,
            _ => return Err(AgentError::Validation("invalid status".into())),
        };
        self.manager.update(id, status);
        Ok(ToolOutput::json(json!({"task_id": id, "status": status_str, "updated": true})))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{TaskCreateTool, TaskGetTool, TaskUpdateTool};
    use crate::knowledge::tasks::TaskManager;
    use crate::tools::api::{Tool, ToolContext};

    #[tokio::test]
    async fn task_tools_persist_windows_path_like_fields_under_nested_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let task_dir = dir.path().join("AppData").join("Local").join("Telos").join("tasks");
        let manager = Arc::new(TaskManager::new(task_dir.clone()));
        let create = TaskCreateTool::new(manager.clone());
        let get = TaskGetTool::new(manager.clone());
        let ctx = ToolContext::dummy();

        let created = create
            .invoke(
                json!({
                    "subject": "Windows path task",
                    "description": r"Check C:\Users\alice\repo before continuing",
                    "blocked_by": [r"%LOCALAPPDATA%\Telos\state.json"]
                }),
                ctx.clone(),
            )
            .await
            .unwrap()
            .content;
        let task_id = created["task_id"].as_str().unwrap();

        assert!(task_dir.join(format!("{task_id}.json")).exists());

        let loaded = get.invoke(json!({"task_id": task_id}), ctx).await.unwrap().content;
        assert_eq!(loaded["description"], r"Check C:\Users\alice\repo before continuing");
        assert_eq!(loaded["blocked_by"][0], r"%LOCALAPPDATA%\Telos\state.json");

        let reopened = TaskManager::new(task_dir);
        let persisted = reopened.get(task_id).unwrap();
        assert_eq!(persisted.blocked_by, vec![r"%LOCALAPPDATA%\Telos\state.json".to_string()]);
    }

    #[tokio::test]
    async fn task_update_rejects_unknown_status() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(TaskManager::new(dir.path().to_path_buf()));
        let update = TaskUpdateTool::new(manager);

        let err = update
            .invoke(json!({"task_id": "task_missing", "status": "blocked"}), ToolContext::dummy())
            .await
            .unwrap_err();

        assert!(err.to_string().contains("invalid status"));
    }
}
