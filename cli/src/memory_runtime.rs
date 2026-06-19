use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use telos_agent::{
    MemoryCategory, MemoryEntry, MemorySection, MemoryStatus, MemoryStore, ToolRegistry, ToolResult,
};

pub fn memory_root(project_root: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = project_root {
        return Ok(root.join(".telos").join("memory"));
    }
    let base = dirs::config_dir().context("could not determine user config directory")?;
    Ok(base.join("telos").join("memory"))
}

pub fn open_memory_store(project_root: Option<&Path>) -> Result<Arc<Mutex<MemoryStore>>> {
    Ok(Arc::new(Mutex::new(MemoryStore::new(memory_root(project_root)?))))
}

pub fn register_memory_runtime(
    tools: &mut ToolRegistry,
    assembly: &mut telos_agent::PromptAssembly,
    store: Arc<Mutex<MemoryStore>>,
) {
    telos_agent::register_memory_tools(tools, store.clone());
    assembly.add(MemorySection::new(store));
}

pub fn record_tool_error(
    store: &Arc<Mutex<MemoryStore>>,
    result: &ToolResult,
    detail: Option<&str>,
) {
    let tool = result.name.clone();
    let label = detail.filter(|d| !d.is_empty()).unwrap_or(&tool);
    let ts = timestamp_now();
    let entry = MemoryEntry {
        name: format!("fix-{}", stable_id(&result.tool_call_id)),
        description: format!("Tool failure: {tool}"),
        category: MemoryCategory::Fact,
        tags: vec!["error".into(), "auto-feedback".into(), tool.to_lowercase()],
        created: ts.clone(),
        updated: ts,
        status: MemoryStatus::NeedsFix,
        times_used: 0,
        confidence: Some("high".into()),
        related: vec![],
        source_session: None,
        body: format!(
            "Tool `{tool}` failed.\n\nDetail: `{label}`\n\nError payload:\n```json\n{}\n```\n\nFix this before retrying.",
            pretty_json(&result.content)
        ),
    };
    upsert_memory(store, entry);
}

pub fn record_successful_tool(
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
    let ts = timestamp_now();
    let category = if matches!(tool.to_lowercase().as_str(), "bash" | "shell") {
        MemoryCategory::Command
    } else {
        MemoryCategory::Workflow
    };
    let entry = MemoryEntry {
        name: format!("tool-{}-{}", tool.to_lowercase(), stable_id(tool_call_id)),
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
    upsert_memory(store, entry);
}

pub fn record_user_preference(store: &Arc<Mutex<MemoryStore>>, prompt: &str) {
    let trimmed = prompt.trim();
    if trimmed.is_empty() || !looks_like_preference(trimmed) {
        return;
    }
    let ts = timestamp_now();
    let entry = MemoryEntry {
        name: format!("preference-{}", stable_id(trimmed)),
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
        body: trimmed.to_string(),
    };
    upsert_memory(store, entry);
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

fn stable_id(input: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}
