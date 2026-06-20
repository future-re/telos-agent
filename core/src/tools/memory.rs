use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

use crate::error::AgentError;
use crate::memory::MemoryEntry;
use crate::memory::MemoryMaintenancePolicy;
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
            .ok_or_else(|| AgentError::Validation("missing `name`".into()))?
            .to_string();
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = store.lock().map_err(|e| AgentError::ToolExecution {
                tool: "MemoryRead".into(),
                message: format!("memory store poisoned: {e}"),
            })?;
            match store.read(&name) {
                Some(entry) => {
                    store.record_use(&name).map_err(|e| AgentError::ToolExecution {
                        tool: "MemoryRead".into(),
                        message: e.to_string(),
                    })?;
                    let mut value = serde_json::to_value(&entry)
                        .unwrap_or(json!({"error":"serialization failed"}));
                    value["body"] = json!(entry.body);
                    Ok(ToolOutput::json(value))
                }
                None => {
                    Ok(ToolOutput::json(json!({"error": format!("memory '{}' not found", name)})))
                }
            }
        })
        .await
        .map_err(|e| AgentError::ToolExecution {
            tool: "MemoryRead".into(),
            message: format!("memory task panicked: {e}"),
        })?
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
                "status":{"type":"string","enum":["working","needs_fix","deprecated"],"default":"working"},
                "confidence":{"type":"string"},
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
            .ok_or(AgentError::Validation("missing name".into()))?
            .to_string();
        let desc = args.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let cat_str = args
            .get("category")
            .and_then(|v| v.as_str())
            .ok_or(AgentError::Validation("missing category".into()))?
            .to_string();
        let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status_str = args.get("status").and_then(|v| v.as_str()).unwrap_or("working");
        let status = match status_str {
            "working" => crate::memory::MemoryStatus::Working,
            "needs_fix" => crate::memory::MemoryStatus::NeedsFix,
            "deprecated" => crate::memory::MemoryStatus::Deprecated,
            _ => return Err(AgentError::Validation(format!("unknown status: {status_str}"))),
        };
        let confidence = args.get("confidence").and_then(|v| v.as_str()).map(String::from);
        let category = match cat_str.as_str() {
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
            name,
            description: desc,
            category,
            tags,
            created: now.clone(),
            updated: now,
            status,
            times_used: 0,
            confidence,
            related,
            source_session: None,
            body,
        };

        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let entry_name = entry.name.clone();
            let mut store = store.lock().map_err(|e| AgentError::ToolExecution {
                tool: "MemoryWrite".into(),
                message: format!("memory store poisoned: {e}"),
            })?;
            store.upsert(entry).map_err(|e| AgentError::ToolExecution {
                tool: "MemoryWrite".into(),
                message: e.to_string(),
            })?;
            Ok(ToolOutput::json(json!({"status": "written", "name": entry_name})))
        })
        .await
        .map_err(|e| AgentError::ToolExecution {
            tool: "MemoryWrite".into(),
            message: format!("memory task panicked: {e}"),
        })?
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
            .ok_or(AgentError::Validation("missing query".into()))?
            .to_string();
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let store = store.lock().map_err(|e| AgentError::ToolExecution {
                tool: "MemoryGrep".into(),
                message: format!("memory store poisoned: {e}"),
            })?;
            let results = store.search(&query);
            let summary: Vec<Value> = results.iter().map(|e| json!({"name": e.name, "description": e.description, "category": format!("{:?}", e.category), "tags": e.tags})).collect();
            Ok(ToolOutput::json(json!({"results": summary, "count": summary.len()})))
        })
        .await
        .map_err(|e| AgentError::ToolExecution {
            tool: "MemoryGrep".into(),
            message: format!("memory task panicked: {e}"),
        })?
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
            .ok_or(AgentError::Validation("missing name".into()))?
            .to_string();
        let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = store.lock().map_err(|e| AgentError::ToolExecution {
                tool: "MemoryEdit".into(),
                message: format!("memory store poisoned: {e}"),
            })?;
            if let Some(mut entry) = store.read(&name) {
                entry.body = body;
                entry.updated = chrono_now();
                store.write(entry).map_err(|e| AgentError::ToolExecution {
                    tool: "MemoryEdit".into(),
                    message: e.to_string(),
                })?;
                Ok(ToolOutput::json(json!({"status": "updated", "name": name})))
            } else {
                Ok(ToolOutput::json(json!({"error": format!("memory '{}' not found", name)})))
            }
        })
        .await
        .map_err(|e| AgentError::ToolExecution {
            tool: "MemoryEdit".into(),
            message: format!("memory task panicked: {e}"),
        })?
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
            .ok_or(AgentError::Validation("missing name".into()))?
            .to_string();
        let status_str = args
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or(AgentError::Validation("missing status".into()))?
            .to_string();
        let status = match status_str.as_str() {
            "working" => crate::memory::MemoryStatus::Working,
            "needs_fix" => crate::memory::MemoryStatus::NeedsFix,
            "deprecated" => crate::memory::MemoryStatus::Deprecated,
            _ => return Err(AgentError::Validation(format!("unknown status: {status_str}"))),
        };
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = store.lock().map_err(|e| AgentError::ToolExecution {
                tool: "MemoryStatus".into(),
                message: format!("memory store poisoned: {e}"),
            })?;
            store.update_status(&name, status).map_err(|e| AgentError::ToolExecution {
                tool: "MemoryStatus".into(),
                message: e.to_string(),
            })?;
            Ok(ToolOutput::json(
                json!({"status": "updated", "name": name, "new_status": status_str}),
            ))
        })
        .await
        .map_err(|e| AgentError::ToolExecution {
            tool: "MemoryStatus".into(),
            message: format!("memory task panicked: {e}"),
        })?
    }
}

// ── MemoryMaintenance ─────────────────────────────────────

pub struct MemoryMaintenanceTool {
    store: Arc<Mutex<MemoryStore>>,
}

impl MemoryMaintenanceTool {
    pub fn new(store: Arc<Mutex<MemoryStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemoryMaintenanceTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "MemoryMaintenance".into(),
            description: "Preview or apply conservative memory cleanup by archiving stale low-value entries. Defaults to dry-run.".into(),
            input_schema: json!({"type":"object","properties":{
                "apply":{"type":"boolean","description":"When true, archive the reported candidates. Defaults to false."},
                "archive_deprecated":{"type":"boolean","description":"Archive deprecated memories. Defaults to true."},
                "max_auto_learned_commands":{"type":"integer","minimum":0,"description":"Keep at most this many active auto-learned command memories. Defaults to 20. Set null to disable this rule."},
                "max_active_entries":{"type":"integer","minimum":0,"description":"Optional cap for active memories overall. Omit or null to disable."}
            }}),
        }
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["memory_maintenance"]
    }
    async fn check_permission(
        &self,
        _: &Value,
        _: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Allow)
    }

    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let apply = args.get("apply").and_then(|v| v.as_bool()).unwrap_or(false);
        let archive_deprecated =
            args.get("archive_deprecated").and_then(|v| v.as_bool()).unwrap_or(true);
        let max_auto_learned_commands = if args.get("max_auto_learned_commands").is_some() {
            optional_usize_arg(&args, "max_auto_learned_commands")?
        } else {
            Some(20)
        };
        let max_active_entries = optional_usize_arg(&args, "max_active_entries")?;
        let policy = MemoryMaintenancePolicy {
            max_auto_learned_commands,
            max_active_entries,
            archive_deprecated,
        };
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = store.lock().map_err(|e| AgentError::ToolExecution {
                tool: "MemoryMaintenance".into(),
                message: format!("memory store poisoned: {e}"),
            })?;
            let report = if apply {
                store.apply_maintenance(&policy).map_err(|e| AgentError::ToolExecution {
                    tool: "MemoryMaintenance".into(),
                    message: e.to_string(),
                })?
            } else {
                store.maintenance_report(&policy)
            };
            let content = serde_json::to_value(report).map_err(|e| AgentError::ToolExecution {
                tool: "MemoryMaintenance".into(),
                message: format!("failed to serialize maintenance report: {e}"),
            })?;
            Ok(ToolOutput::json(content))
        })
        .await
        .map_err(|e| AgentError::ToolExecution {
            tool: "MemoryMaintenance".into(),
            message: format!("memory task panicked: {e}"),
        })?
    }
}

fn optional_usize_arg(args: &Value, name: &str) -> Result<Option<usize>, AgentError> {
    let Some(value) = args.get(name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(n) = value.as_u64() else {
        return Err(AgentError::Validation(format!("`{name}` must be a non-negative integer")));
    };
    usize::try_from(n)
        .map(Some)
        .map_err(|_| AgentError::Validation(format!("`{name}` is too large")))
}

fn is_leap_year(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn chrono_now() -> String {
    // Simple date string without chrono dep.
    // Walk forward from Unix epoch to find the current year, then month.
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let mut days = now.as_secs() / 86400;
    let mut year: u64 = 1970;
    loop {
        let days_in_year: u64 = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let ml: [u64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let (mut rem, mut m) = (days, 1usize);
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
