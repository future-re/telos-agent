use serde_json::{Value, json};

use crate::error::AgentError;
use crate::subagent::tool::SubagentTool;
use crate::subagent::{ForkLens, ForkResult, ForkShared, Synapse};
use crate::tool::{ToolContext, ToolOutput};

impl SubagentTool {
    /// Execute a fork run: run each lens through the provider concurrently.
    pub(super) async fn run_fork(
        &self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let forks = arguments
            .get("forks")
            .and_then(|f| f.as_array())
            .ok_or_else(|| AgentError::Validation("fork mode requires `forks` array".into()))?;

        let lenses: Vec<ForkLens> = forks
            .iter()
            .filter_map(|item| {
                let lens = item.get("lens")?.as_str()?;
                let system_prompt = item.get("system_prompt")?.as_str()?;
                let task = item.get("task")?.as_str()?;
                Some(ForkLens {
                    lens: lens.to_string(),
                    system_prompt: system_prompt.to_string(),
                    task: task.to_string(),
                    output_schema: item.get("output_schema").cloned(),
                    allowed_tools: item
                        .get("allowed_tools")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                })
            })
            .collect();

        if lenses.is_empty() {
            return Err(AgentError::Validation(
                "fork mode requires at least one lens with `lens`, `system_prompt`, and `task`"
                    .into(),
            ));
        }

        let fork_shared = ForkShared {
            provider: self.provider.clone(),
            tool_registry: self.tools.clone(),
            messages: context.messages.clone(),
            config: self.config.clone(),
        };

        let synapse = Synapse::new(4);
        let execution = synapse.run_all(&fork_shared, lenses, None).await;

        let results: Vec<Value> = execution
            .results
            .iter()
            .map(|result| match result {
                Some(ForkResult::Text(text)) => json!({ "text": text }),
                Some(ForkResult::Structured(value)) => {
                    json!({ "structured": value, "text": value.to_string() })
                }
                None => json!({ "error": "lens execution failed" }),
            })
            .collect();

        Ok(ToolOutput::json(json!({
            "mode": "fork",
            "lens_count": results.len(),
            "results": results,
        })))
    }
}
