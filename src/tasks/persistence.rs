use crate::tasks::task::Task;
use std::path::Path;

pub fn save(dir: &Path, task: &Task) -> std::io::Result<()> {
    let path = dir.join(format!("{}.json", task.id));
    let json = serde_json::to_string_pretty(task)?;
    std::fs::write(path, json)
}
