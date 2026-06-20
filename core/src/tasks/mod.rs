mod persistence;
pub mod task;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
pub use task::{Task, TaskStatus};

use crate::config::CancellationState;

// Compatibility re-export: the task tool implementations now live in
// `crate::tools` so they live next to the other built-in tools.
pub use crate::tools::{
    TaskCreateTool, TaskGetTool, TaskListTool, TaskOutputTool, TaskStopTool, TaskUpdateTool,
};

pub struct TaskManager {
    tasks: Mutex<HashMap<String, Task>>,
    cancellations: Mutex<HashMap<String, CancellationState>>,
    dir: PathBuf,
}

impl TaskManager {
    pub fn new(dir: PathBuf) -> Self {
        std::fs::create_dir_all(&dir).ok();
        let tasks =
            persistence::load_all(&dir).into_iter().map(|task| (task.id.clone(), task)).collect();
        Self { tasks: Mutex::new(tasks), cancellations: Mutex::new(HashMap::new()), dir }
    }
    pub fn create(&self, task: Task) {
        let id = task.id.clone();
        self.tasks.lock().unwrap().insert(id.clone(), task.clone());
        let _ = persistence::save(&self.dir, &task);
    }
    pub fn get(&self, id: &str) -> Option<Task> {
        self.tasks.lock().unwrap().get(id).cloned()
    }
    pub fn list(&self) -> Vec<Task> {
        self.tasks.lock().unwrap().values().cloned().collect()
    }
    pub fn update(&self, id: &str, status: TaskStatus) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(id) {
            task.status = status;
            let task = task.clone();
            drop(tasks);
            let _ = persistence::save(&self.dir, &task);
        }
    }

    pub fn update_task(&self, id: &str, update: impl FnOnce(&mut Task)) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(id) {
            update(task);
            let task = task.clone();
            drop(tasks);
            let _ = persistence::save(&self.dir, &task);
        }
    }

    pub fn set_output(&self, id: &str, output: String) {
        self.update_task(id, |task| {
            task.output = Some(output);
        });
    }

    pub fn complete(&self, id: &str, output: Option<String>) {
        self.update_task(id, |task| {
            task.status = TaskStatus::Completed;
            task.error = None;
            if let Some(output) = output {
                task.output = Some(output);
            }
        });
    }

    pub fn fail(&self, id: &str, error: String) {
        self.update_task(id, |task| {
            task.status = TaskStatus::Failed;
            task.error = Some(error);
        });
    }

    pub fn cancel(&self, id: &str, reason: String) {
        if let Some(cancellation) = self.cancellations.lock().unwrap().get(id).cloned() {
            cancellation.cancel();
        }
        self.update_task(id, |task| {
            task.status = TaskStatus::Cancelled;
            task.error = Some(reason);
        });
    }

    pub fn register_cancellation(&self, id: impl Into<String>, cancellation: CancellationState) {
        self.cancellations.lock().unwrap().insert(id.into(), cancellation);
    }

    pub fn unregister_cancellation(&self, id: &str) {
        self.cancellations.lock().unwrap().remove(id);
    }

    pub fn request_cancel(&self, id: &str) -> bool {
        if let Some(cancellation) = self.cancellations.lock().unwrap().get(id).cloned() {
            cancellation.cancel();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_loads_existing_persisted_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let task = Task {
            id: "task_existing".into(),
            subject: "Persisted task".into(),
            description: "Loaded on startup".into(),
            status: TaskStatus::InProgress,
            blocked_by: vec![],
            blocks: vec![],
            output: None,
            kind: None,
            agent_id: None,
            agent_type: None,
            worktree_path: None,
            error: None,
        };
        persistence::save(dir.path(), &task).unwrap();

        let manager = TaskManager::new(dir.path().to_path_buf());

        assert_eq!(manager.get("task_existing").unwrap().subject, "Persisted task");
        assert_eq!(manager.list().len(), 1);
    }

    #[test]
    fn new_ignores_malformed_and_non_json_task_files() {
        let dir = tempfile::tempdir().unwrap();
        let task = Task {
            id: "task_valid".into(),
            subject: "Valid task".into(),
            description: "Loaded from valid JSON".into(),
            status: TaskStatus::Pending,
            blocked_by: vec![],
            blocks: vec![],
            output: None,
            kind: None,
            agent_id: None,
            agent_type: None,
            worktree_path: None,
            error: None,
        };
        persistence::save(dir.path(), &task).unwrap();
        std::fs::write(dir.path().join("bad.json"), "{not valid json").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "ignore me").unwrap();

        let manager = TaskManager::new(dir.path().to_path_buf());

        assert_eq!(manager.list().len(), 1);
        assert_eq!(manager.get("task_valid").unwrap().subject, "Valid task");
    }
}
