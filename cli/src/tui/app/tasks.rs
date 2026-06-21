use std::path::{Path, PathBuf};

pub(super) fn task_dir_for_root(root: &Path) -> PathBuf {
    root.join(".telos").join("tasks")
}

pub(super) fn tasks_in_dir(task_dir: &Path) -> Vec<telos_agent::Task> {
    let Ok(entries) = std::fs::read_dir(task_dir) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            if !entry.file_type().ok()?.is_file() {
                return None;
            }
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                return None;
            }
            let content = std::fs::read_to_string(path).ok()?;
            serde_json::from_str(&content).ok()
        })
        .collect()
}

pub(super) fn format_task_summary(tasks: &[telos_agent::Task]) -> String {
    if tasks.is_empty() {
        return "No persisted tasks found.".to_string();
    }

    let mut tasks = tasks.to_vec();
    tasks.sort_by(|a, b| {
        task_status_rank(&a.status)
            .cmp(&task_status_rank(&b.status))
            .then_with(|| a.subject.cmp(&b.subject))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut lines = vec!["Persisted tasks:".to_string(), String::new()];
    for task in tasks {
        let blocked_by = if task.blocked_by.is_empty() {
            String::new()
        } else {
            format!(" (blocked by {})", task.blocked_by.join(", "))
        };
        lines.push(format!(
            "  {} {} [{}] {}{}",
            task_status_marker(&task.status),
            task.subject,
            task_status_label(&task.status),
            task.id,
            blocked_by
        ));
    }
    lines.join("\n")
}

fn task_status_rank(status: &telos_agent::TaskStatus) -> u8 {
    match status {
        telos_agent::TaskStatus::Pending => 0,
        telos_agent::TaskStatus::InProgress => 1,
        telos_agent::TaskStatus::Completed => 2,
        telos_agent::TaskStatus::Failed => 3,
        telos_agent::TaskStatus::Cancelled => 4,
        telos_agent::TaskStatus::Deleted => 5,
    }
}

fn task_status_label(status: &telos_agent::TaskStatus) -> &'static str {
    match status {
        telos_agent::TaskStatus::Pending => "pending",
        telos_agent::TaskStatus::InProgress => "in_progress",
        telos_agent::TaskStatus::Completed => "completed",
        telos_agent::TaskStatus::Failed => "failed",
        telos_agent::TaskStatus::Cancelled => "cancelled",
        telos_agent::TaskStatus::Deleted => "deleted",
    }
}

fn task_status_marker(status: &telos_agent::TaskStatus) -> &'static str {
    match status {
        telos_agent::TaskStatus::Pending => "◦",
        telos_agent::TaskStatus::InProgress => "•",
        telos_agent::TaskStatus::Completed => "✓",
        telos_agent::TaskStatus::Failed => "!",
        telos_agent::TaskStatus::Cancelled => "-",
        telos_agent::TaskStatus::Deleted => "×",
    }
}

#[cfg(test)]
mod tests {
    use super::{format_task_summary, tasks_in_dir};

    #[test]
    fn reads_without_creating_or_accepting_invalid_entries() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join(".telos").join("tasks");

        assert!(tasks_in_dir(&missing).is_empty());
        assert!(!missing.exists());

        std::fs::create_dir_all(&missing).unwrap();
        let valid = telos_agent::Task {
            id: "task_valid".into(),
            subject: "Visible".into(),
            description: "Shown by /tasks".into(),
            status: telos_agent::TaskStatus::Pending,
            blocked_by: vec![],
            blocks: vec![],
            output: None,
            kind: None,
            agent_id: None,
            agent_type: None,
            worktree_path: None,
            error: None,
        };
        std::fs::write(
            missing.join("task_valid.json"),
            serde_json::to_string_pretty(&valid).unwrap(),
        )
        .unwrap();
        std::fs::write(missing.join("bad.json"), "{not valid json").unwrap();
        std::fs::write(missing.join("notes.txt"), "ignore me").unwrap();
        std::fs::create_dir(missing.join("directory-task.json")).unwrap();

        let tasks = tasks_in_dir(&missing);

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "task_valid");
    }

    #[test]
    fn formats_empty_and_grouped_tasks() {
        assert_eq!(format_task_summary(&[]), "No persisted tasks found.");

        let tasks = vec![
            telos_agent::Task {
                id: "task_a".into(),
                subject: "Write tests".into(),
                description: "Cover task list rendering".into(),
                status: telos_agent::TaskStatus::Pending,
                blocked_by: vec![],
                blocks: vec![],
                output: None,
                kind: None,
                agent_id: None,
                agent_type: None,
                worktree_path: None,
                error: None,
            },
            telos_agent::Task {
                id: "task_b".into(),
                subject: "Implement command".into(),
                description: "Add /tasks".into(),
                status: telos_agent::TaskStatus::InProgress,
                blocked_by: vec!["task_a".into()],
                blocks: vec![],
                output: None,
                kind: None,
                agent_id: None,
                agent_type: None,
                worktree_path: None,
                error: None,
            },
            telos_agent::Task {
                id: "task_c".into(),
                subject: "Commit".into(),
                description: "Save changes".into(),
                status: telos_agent::TaskStatus::Completed,
                blocked_by: vec![],
                blocks: vec![],
                output: None,
                kind: None,
                agent_id: None,
                agent_type: None,
                worktree_path: None,
                error: None,
            },
        ];

        assert_eq!(
            format_task_summary(&tasks),
            "Persisted tasks:\n\n  ◦ Write tests [pending] task_a\n  • Implement command [in_progress] task_b (blocked by task_a)\n  ✓ Commit [completed] task_c"
        );
    }
}
