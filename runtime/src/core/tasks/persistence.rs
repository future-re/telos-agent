use crate::tasks::task::Task;
use std::path::Path;

pub fn save(dir: &Path, task: &Task) -> std::io::Result<()> {
    let path = dir.join(format!("{}.json", task.id));
    let json = serde_json::to_string_pretty(task)?;
    std::fs::write(path, json)
}

pub fn load_all(dir: &Path) -> Vec<Task> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                return None;
            }
            let content = std::fs::read_to_string(path).ok()?;
            serde_json::from_str(&content).ok()
        })
        .collect()
}
