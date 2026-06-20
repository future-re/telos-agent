use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use telos_agent::memory::unix_timestamp;
use telos_agent::{
    MemoryCategory, MemoryEntry, MemoryMaintenancePolicy, MemorySection, MemoryStatus, MemoryStore,
    ToolRegistry, ToolResult,
};

pub fn memory_root(project_root: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = project_root {
        return Ok(root.join(".telos").join("memory"));
    }
    let base = dirs::config_dir().context("could not determine user config directory")?;
    Ok(base.join("telos").join("memory"))
}

pub fn open_memory_store(project_root: Option<&Path>) -> Result<Arc<Mutex<MemoryStore>>> {
    let mut store = MemoryStore::new(memory_root(project_root)?);
    match store.apply_maintenance(&MemoryMaintenancePolicy::default()) {
        Ok(report) if report.archived_count > 0 => {
            tracing::info!(
                archived_count = report.archived_count,
                candidates = report.actions.len(),
                "memory maintenance archived stale entries"
            );
        }
        Ok(_) => {}
        Err(err) => tracing::warn!("memory maintenance failed: {err}"),
    }
    Ok(Arc::new(Mutex::new(store)))
}

pub fn register_memory_runtime(
    tools: &mut ToolRegistry,
    assembly: &mut telos_agent::PromptAssembly,
    store: Arc<Mutex<MemoryStore>>,
) {
    telos_agent::register_memory_tools(tools, store.clone());
    assembly.add(MemorySection::new(store));
}

pub async fn record_tool_error(
    store: &Arc<Mutex<MemoryStore>>,
    result: &ToolResult,
    detail: Option<&str>,
) {
    let store = store.clone();
    // Extract only the fields we need, avoiding a full ToolResult clone.
    let tool_name = result.name.clone();
    let tool_call_id = result.tool_call_id.clone();
    let content = result.content.clone();
    let detail = detail.map(String::from);
    if let Err(e) = tokio::task::spawn_blocking(move || {
        let label = detail.filter(|d| !d.is_empty()).unwrap_or_else(|| tool_name.clone());
        let ts = unix_timestamp();
        let entry = MemoryEntry {
            name: format!("tool-error-{}", stable_id(&tool_call_id)),
            description: format!("Tool execution failed: {tool_name}"),
            category: MemoryCategory::Fact,
            tags: vec!["tool-error".into(), "auto-feedback".into(), tool_name.to_lowercase()],
            created: ts.clone(),
            updated: ts,
            status: MemoryStatus::Working,
            times_used: 0,
            confidence: Some("high".into()),
            related: vec![],
            source_session: None,
            body: format!(
                "Tool `{tool_name}` execution failed.\n\nDetail: `{label}`\n\nError payload:\n```json\n{}\n```\n\nThis is execution context for future runs, not a bug-fix task.",
                pretty_json(&content)
            ),
        };
        upsert_memory(&store, entry);
    })
    .await
    {
        tracing::warn!("record_tool_error spawn_blocking failed: {e}");
    }
}

pub async fn record_successful_tool(
    store: &Arc<Mutex<MemoryStore>>,
    tool: &str,
    tool_call_id: &str,
    detail: Option<&str>,
) {
    let Some(detail) = detail.filter(|d| !d.trim().is_empty()) else {
        return;
    };
    if !matches!(tool.to_lowercase().as_str(), "bash" | "shell" | "edit" | "write") {
        return;
    }
    let store = store.clone();
    let tool = tool.to_string();
    let tool_call_id = tool_call_id.to_string();
    let detail = detail.to_string();
    if let Err(e) = tokio::task::spawn_blocking(move || {
        let ts = unix_timestamp();
        let category = if matches!(tool.to_lowercase().as_str(), "bash" | "shell") {
            MemoryCategory::Command
        } else {
            MemoryCategory::Workflow
        };
        let entry = MemoryEntry {
            name: format!("tool-{}-{}", tool.to_lowercase(), stable_id(&tool_call_id)),
            description: format!("Successful {tool} usage: {detail}"),
            category,
            tags: vec!["auto-learned".into(), tool.to_lowercase()],
            created: ts.clone(),
            updated: ts,
            status: MemoryStatus::Working,
            times_used: 0,
            confidence: Some("medium".into()),
            related: vec![],
            source_session: None,
            body: format!("Tool `{tool}` completed successfully.\n\nDetail: `{detail}`"),
        };
        upsert_memory(&store, entry);
    })
    .await
    {
        tracing::warn!("record_successful_tool spawn_blocking failed: {e}");
    }
}

pub async fn record_subagent_learning(store: &Arc<Mutex<MemoryStore>>, result: &ToolResult) {
    if result.is_error || !is_subagent_result(result) {
        return;
    }
    let Some(final_text) = result.content.get("final_text").and_then(|value| value.as_str()) else {
        return;
    };
    let Some(learning) = extract_reusable_learning(final_text) else {
        return;
    };
    let store = store.clone();
    let tool_call_id = result.tool_call_id.clone();
    let agent_type = result
        .content
        .get("agent_type")
        .and_then(|value| value.as_str())
        .unwrap_or("subagent")
        .to_string();
    let description = result
        .content
        .get("description")
        .and_then(|value| value.as_str())
        .unwrap_or("delegated task")
        .to_string();
    if let Err(e) = tokio::task::spawn_blocking(move || {
        let ts = unix_timestamp();
        let entry = MemoryEntry {
            name: format!("subagent-learning-{}", stable_id(&format!("{tool_call_id}:{learning}"))),
            description: format!("Subagent learning from {agent_type}: {description}"),
            category: MemoryCategory::Workflow,
            tags: vec!["auto-learned".into(), "subagent".into(), agent_type.to_lowercase()],
            created: ts.clone(),
            updated: ts,
            status: MemoryStatus::Working,
            times_used: 0,
            confidence: Some("medium".into()),
            related: vec![],
            source_session: None,
            body: learning,
        };
        upsert_memory(&store, entry);
    })
    .await
    {
        tracing::warn!("record_subagent_learning spawn_blocking failed: {e}");
    }
}

pub async fn record_user_preference(store: &Arc<Mutex<MemoryStore>>, prompt: &str) {
    let trimmed = prompt.trim();
    if trimmed.is_empty() || !looks_like_preference(trimmed) {
        return;
    }
    let store = store.clone();
    let trimmed = trimmed.to_string();
    if let Err(e) = tokio::task::spawn_blocking(move || {
        let ts = unix_timestamp();
        let entry = MemoryEntry {
            name: format!("preference-{}", stable_id(&trimmed)),
            description: "User preference".into(),
            category: MemoryCategory::Fact,
            tags: vec!["auto-learned".into(), "preference".into()],
            created: ts.clone(),
            updated: ts,
            status: MemoryStatus::Working,
            times_used: 0,
            confidence: Some("high".into()),
            related: vec![],
            source_session: None,
            body: trimmed,
        };
        upsert_memory(&store, entry);
    })
    .await
    {
        tracing::warn!("record_user_preference spawn_blocking failed: {e}");
    }
}

fn upsert_memory(store: &Arc<Mutex<MemoryStore>>, entry: MemoryEntry) {
    match store.lock() {
        Ok(mut store) => {
            if let Err(err) = store.upsert(entry) {
                tracing::warn!("failed to write memory: {err}");
            }
        }
        Err(err) => tracing::warn!("failed to lock memory store: {err}"),
    }
}

fn looks_like_preference(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("remember")
        || lower.contains("always")
        || lower.contains("prefer")
        || text.contains("记住")
        || text.contains("以后")
        || text.contains("偏好")
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn is_subagent_result(result: &ToolResult) -> bool {
    result.name == "subagent"
        || result.content.get("kind").and_then(|value| value.as_str()) == Some("subagent")
        || result.content.get("agent_id").is_some()
}

fn extract_reusable_learning(text: &str) -> Option<String> {
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.iter().position(|line| {
        let normalized =
            line.trim().trim_matches('#').trim().trim_end_matches(':').to_ascii_lowercase();
        normalized == "reusable learning"
    })?;
    let mut collected = Vec::new();
    for line in lines.iter().skip(start + 1) {
        let trimmed = line.trim();
        let is_next_heading = trimmed.starts_with('#')
            || (trimmed.ends_with(':')
                && !trimmed.starts_with('-')
                && !trimmed.eq_ignore_ascii_case("reusable learning:"));
        if is_next_heading && !collected.is_empty() {
            break;
        }
        collected.push(*line);
    }
    let learning = collected.join("\n").trim().to_string();
    if learning.is_empty() { None } else { Some(learning) }
}

fn stable_id(input: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto_command(name: &str, updated: &str) -> MemoryEntry {
        MemoryEntry {
            name: name.into(),
            description: format!("Auto command {name}"),
            category: MemoryCategory::Command,
            tags: vec!["auto-learned".into(), "bash".into()],
            created: updated.into(),
            updated: updated.into(),
            status: MemoryStatus::Working,
            times_used: 0,
            confidence: Some("medium".into()),
            related: vec![],
            source_session: None,
            body: format!("body {name}"),
        }
    }

    #[test]
    fn open_memory_store_applies_default_maintenance() {
        let project = tempfile::tempdir().unwrap();
        let root = memory_root(Some(project.path())).unwrap();
        let mut seed = MemoryStore::new(root.clone());
        for n in 0..21 {
            seed.write(auto_command(&format!("auto-{n:02}"), &format!("{n:03}"))).unwrap();
        }

        let store = open_memory_store(Some(project.path())).unwrap();
        let store = store.lock().unwrap();

        assert_eq!(store.list().len(), 20);
        assert!(store.read("auto-00").is_none());
        assert!(store.read("auto-20").is_some());
        assert!(root.join("_archived").join("auto-00.md").exists());
    }

    #[tokio::test]
    async fn record_subagent_learning_writes_reusable_learning_section() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(MemoryStore::new(dir.path().to_path_buf())));
        let result = ToolResult {
            tool_call_id: "call-1".into(),
            name: "subagent".into(),
            is_error: false,
            content: serde_json::json!({
                "agent_id": "agent_explore_1",
                "agent_type": "Explore",
                "description": "Explore parser",
                "final_text": "Outcome: done\n\nReusable learning:\n- Parser tests live in core/tests/parser_tests.rs.\n- Use cargo test -p telos_agent parser."
            }),
        };

        record_subagent_learning(&store, &result).await;

        let store = store.lock().unwrap();
        let memories = store.list();
        assert_eq!(memories.len(), 1);
        let memory = store.read(&memories[0]).expect("memory should be readable");
        assert_eq!(memory.category, MemoryCategory::Workflow);
        assert!(memory.tags.contains(&"subagent".into()));
        assert!(memory.body.contains("Parser tests live"));
        assert!(!memory.body.contains("Outcome: done"));
    }

    #[tokio::test]
    async fn record_subagent_learning_ignores_plain_subagent_output() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(MemoryStore::new(dir.path().to_path_buf())));
        let result = ToolResult {
            tool_call_id: "call-1".into(),
            name: "subagent".into(),
            is_error: false,
            content: serde_json::json!({
                "agent_id": "agent_explore_1",
                "agent_type": "Explore",
                "final_text": "Outcome: done"
            }),
        };

        record_subagent_learning(&store, &result).await;

        assert!(store.lock().unwrap().list().is_empty());
    }
}
