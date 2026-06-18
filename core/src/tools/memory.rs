use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

use crate::error::AgentError;
use crate::memory::MemoryEntry;
use crate::memory::MemoryStore;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

// ── MemoryRead ───────────────────────────────────────────

pub struct MemoryReadTool {
    store: Arc<Mutex<MemoryStore>>,
}

impl MemoryReadTool {
    pub fn new(store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemoryReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "MemoryRead".into(),
            description: "Read a memory entry by name. Returns the full content including body."
                .into(),
            input_schema: json!({"type":"object","properties":{"name":{"type":"string","description":"Name of the memory to read"}},"required":["name"]}),
        }
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["memory_read"]
    }
    async fn check_permission(
        &self,
        _: &Value,
        _: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Allow)
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::Validation("missing `name`".into()))?;
        let store = self.store.lock().unwrap();
        match store.read(name) {
            Some(entry) => {
                let mut value =
                    serde_json::to_value(&entry).unwrap_or(json!({"error":"serialization failed"}));
                value["body"] = json!(entry.body);
                Ok(ToolOutput::json(value))
            }
            None => Ok(ToolOutput::json(json!({"error": format!("memory '{}' not found", name)}))),
        }
    }
}

// ── MemoryWrite ──────────────────────────────────────────

pub struct MemoryWriteTool {
    store: Arc<Mutex<MemoryStore>>,
}

impl MemoryWriteTool {
    pub fn new(store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemoryWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "MemoryWrite".into(),
            description:
                "Write a new memory entry. Categories: script, command, pattern, fact, workflow."
                    .into(),
            input_schema: json!({"type":"object","properties":{
                "name":{"type":"string"},
                "description":{"type":"string"},
                "category":{"type":"string","enum":["script","command","pattern","fact","workflow"]},
                "body":{"type":"string","description":"Markdown body content"},
                "tags":{"type":"array","items":{"type":"string"},"default":[]},
                "related":{"type":"array","items":{"type":"string"},"default":[]}
            },"required":["name","description","category","body"]}),
        }
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["memory_write"]
    }
    async fn check_permission(
        &self,
        _: &Value,
        _: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Allow)
    }

    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or(AgentError::Validation("missing name".into()))?;
        let desc = args.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let cat_str = args
            .get("category")
            .and_then(|v| v.as_str())
            .ok_or(AgentError::Validation("missing category".into()))?;
        let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let category = match cat_str {
            "script" => crate::memory::MemoryCategory::Script,
            "command" => crate::memory::MemoryCategory::Command,
            "pattern" => crate::memory::MemoryCategory::Pattern,
            "fact" => crate::memory::MemoryCategory::Fact,
            "workflow" => crate::memory::MemoryCategory::Workflow,
            _ => return Err(AgentError::Validation(format!("unknown category: {cat_str}"))),
        };
        let tags: Vec<String> = args
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let related: Vec<String> = args
            .get("related")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let now = chrono_now();

        let entry = MemoryEntry {
            name: name.to_string(),
            description: desc.to_string(),
            category,
            tags,
            created: now.clone(),
            updated: now,
            status: crate::memory::MemoryStatus::Working,
            times_used: 0,
            confidence: None,
            related,
            source_session: None,
            body: body.to_string(),
        };

        let mut store = self.store.lock().unwrap();
        store.write(entry).map_err(|e| AgentError::ToolExecution {
            tool: "MemoryWrite".into(),
            message: e.to_string(),
        })?;
        Ok(ToolOutput::json(json!({"status": "written", "name": name})))
    }
}

// ── MemoryGrep ───────────────────────────────────────────

pub struct MemoryGrepTool {
    store: Arc<Mutex<MemoryStore>>,
}

impl MemoryGrepTool {
    pub fn new(store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemoryGrepTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "MemoryGrep".into(),
            description:
                "Search memories by keyword. Matches against name, description, tags, and body."
                    .into(),
            input_schema: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        }
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["memory_grep"]
    }
    async fn check_permission(
        &self,
        _: &Value,
        _: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Allow)
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or(AgentError::Validation("missing query".into()))?;
        let store = self.store.lock().unwrap();
        let results = store.search(query);
        let summary: Vec<Value> = results.iter().map(|e| json!({"name": e.name, "description": e.description, "category": format!("{:?}", e.category), "tags": e.tags})).collect();
        Ok(ToolOutput::json(json!({"results": summary, "count": summary.len()})))
    }
}

// ── MemoryEdit ───────────────────────────────────────────

pub struct MemoryEditTool {
    store: Arc<Mutex<MemoryStore>>,
}

impl MemoryEditTool {
    pub fn new(store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemoryEditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "MemoryEdit".into(),
            description: "Edit a memory entry's body content. Replaces the full body.".into(),
            input_schema: json!({"type":"object","properties":{"name":{"type":"string"},"body":{"type":"string","description":"New body content"}},"required":["name","body"]}),
        }
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["memory_edit"]
    }
    async fn check_permission(
        &self,
        _: &Value,
        _: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Allow)
    }

    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or(AgentError::Validation("missing name".into()))?;
        let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let mut store = self.store.lock().unwrap();
        if let Some(mut entry) = store.read(name) {
            entry.body = body.to_string();
            entry.updated = chrono_now();
            store.write(entry).map_err(|e| AgentError::ToolExecution {
                tool: "MemoryEdit".into(),
                message: e.to_string(),
            })?;
            Ok(ToolOutput::json(json!({"status": "updated", "name": name})))
        } else {
            Ok(ToolOutput::json(json!({"error": format!("memory '{}' not found", name)})))
        }
    }
}

// ── MemoryStatus ─────────────────────────────────────────

pub struct MemoryStatusTool {
    store: Arc<Mutex<MemoryStore>>,
}

impl MemoryStatusTool {
    pub fn new(store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemoryStatusTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "MemoryStatus".into(),
            description: "Update the status of a memory entry: working, needs_fix, or deprecated."
                .into(),
            input_schema: json!({"type":"object","properties":{"name":{"type":"string"},"status":{"type":"string","enum":["working","needs_fix","deprecated"]}},"required":["name","status"]}),
        }
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["memory_status"]
    }
    async fn check_permission(
        &self,
        _: &Value,
        _: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Allow)
    }

    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or(AgentError::Validation("missing name".into()))?;
        let status_str = args
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or(AgentError::Validation("missing status".into()))?;
        let status = match status_str {
            "working" => crate::memory::MemoryStatus::Working,
            "needs_fix" => crate::memory::MemoryStatus::NeedsFix,
            "deprecated" => crate::memory::MemoryStatus::Deprecated,
            _ => return Err(AgentError::Validation(format!("unknown status: {status_str}"))),
        };
        let mut store = self.store.lock().unwrap();
        store.update_status(name, status).map_err(|e| AgentError::ToolExecution {
            tool: "MemoryStatus".into(),
            message: e.to_string(),
        })?;
        Ok(ToolOutput::json(json!({"status": "updated", "name": name, "new_status": status_str})))
    }
}

fn chrono_now() -> String {
    // Simple date string without chrono dep
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let days_since_2024 = secs.saturating_sub(1704067200) / 86400;
    let year = 2024 + (days_since_2024 / 365);
    let day_of_year = days_since_2024 % 365;
    let ml = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let (mut rem, mut m) = (day_of_year, 1usize);
    for &l in &ml {
        if rem < l {
            break;
        }
        rem -= l;
        m += 1;
    }
    let d = rem + 1;
    format!("{year}-{m:02}-{d:02}")
}
