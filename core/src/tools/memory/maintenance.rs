use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

use crate::error::AgentError;
use crate::memory::{MemoryMaintenancePolicy, MemoryStore};
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

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
