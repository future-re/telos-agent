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
        Self { tasks: Mutex::new(HashMap::new()), dir }
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
