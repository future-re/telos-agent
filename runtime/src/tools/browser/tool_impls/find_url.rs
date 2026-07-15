use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};
use crate::tools::browser::util::*;

pub struct BrowserFindUrlTool;

#[async_trait]
impl Tool for BrowserFindUrlTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserFindUrl".into(),
            description: "Search local browser bookmarks/history metadata for likely URLs. Requires explicit approval.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "default": 10 }
                },
                "required": ["query"]
            }),
        }
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Ask {
            reason: "reading local browser bookmarks/history metadata requires approval".into(),
        })
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let query = arguments
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::Validation("missing string `query`".into()))?;
        let limit = optional_u32(&arguments, "limit").unwrap_or(10).min(50) as usize;
        let mut results = Vec::new();
        for path in candidate_bookmark_paths() {
            if results.len() >= limit {
                break;
            }
            let Ok(content) = tokio::fs::read_to_string(&path).await else {
                continue;
            };
            collect_bookmark_matches(&content, query, limit, &mut results);
        }
        Ok(ToolOutput::json(json!({
            "query": query,
            "count": results.len(),
            "results": results,
            "note": "Only bookmark metadata is read in v1; browser history databases are intentionally not opened yet."
        })))
    }
}
