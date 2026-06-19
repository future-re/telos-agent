mod persistence;
pub mod task;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
pub use task::{Task, TaskStatus};

// Compatibility re-export: the task tool implementations now live in
// `crate::tools` so they live next to the other built-in tools.
pub use crate::tools::{TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool};

pub struct TaskManager {
    tasks: Mutex<HashMap<String, Task>>,
    dir: PathBuf,
}

impl TaskManager {
    pub fn new(dir: PathBuf) -> Self {
        std::fs::create_dir_all(&dir).ok();
        let tasks =
            persistence::load_all(&dir).into_iter().map(|task| (task.id.clone(), task)).collect();
        Self { tasks: Mutex::new(tasks), dir }
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
        if let Some(task) = self.tasks.lock().unwrap().get_mut(id) {
            task.status = status;
            let _ = persistence::save(&self.dir, task);
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
        };
        persistence::save(dir.path(), &task).unwrap();
        std::fs::write(dir.path().join("bad.json"), "{not valid json").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "ignore me").unwrap();

        let manager = TaskManager::new(dir.path().to_path_buf());

        assert_eq!(manager.list().len(), 1);
        assert_eq!(manager.get("task_valid").unwrap().subject, "Valid task");
    }
}
