//! `ExitPlanMode` tool — submit an implementation plan for approval.
//!
//! Reads the plan from the file written by the model while in plan mode.
//! Returns the plan content for user/leader approval. When approved, the
//! agent exits plan mode and can resume normal write-capable operations.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

use super::enter_plan_mode::SharedPlanState;

/// `ExitPlanMode` — read the plan file and submit it.
pub struct ExitPlanModeTool {
    plan_state: SharedPlanState,
}

impl ExitPlanModeTool {
    pub fn new(plan_state: SharedPlanState) -> Self {
        Self { plan_state }
    }
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ExitPlanMode".into(),
            description: "Submit the implementation plan for approval. Reads the plan from disk \
(e.g. the plan.md file you wrote) and presents it. Call this after writing the plan \
file with FileWrite. Do NOT use for exploration-only tasks — only when you have a \
written plan to submit."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "plan": {
                        "type": "string",
                        "description": "The full plan text (as an alternative to writing to a file)."
                    }
                },
                "additionalProperties": true
            }),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(EXIT_PLAN_MODE_PROMPT)
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        // Check if plan mode is actually active
        let plan_file_path = {
            let state = self.plan_state.lock().unwrap();
            if !state.active {
                return Err(AgentError::Validation(
                    "ExitPlanMode called outside plan mode. Use EnterPlanMode first.".into(),
                ));
            }
            state.plan_file_path.clone()
        };

        // Try reading from the inline `plan` argument first
        let plan_text = if let Some(inline_plan) = arguments.get("plan").and_then(|v| v.as_str()) {
            inline_plan.to_string()
        } else if let Some(ref plan_path) = plan_file_path {
            // Read plan from disk
            match std::fs::read_to_string(plan_path) {
                Ok(content) if content.trim().is_empty() => {
                    return Err(AgentError::Validation(format!(
                        "Plan file at {} is empty. Write a plan before calling ExitPlanMode.",
                        plan_path.display()
                    )));
                }
                Ok(content) => content,
                Err(e) => {
                    return Err(AgentError::Validation(format!(
                        "Could not read plan file at {}: {}. Write the plan to this file (using FileWrite) before calling ExitPlanMode, or pass the plan text inline.",
                        plan_path.display(),
                        e
                    )));
                }
            }
        } else {
            return Err(AgentError::Validation(
                "No plan file path configured and no inline `plan` argument provided.".into(),
            ));
        };

        // Deactivate plan mode
        {
            let mut state = self.plan_state.lock().unwrap();
            state.active = false;
        }

        Ok(ToolOutput::json(json!({
            "plan": plan_text,
            "message": "Plan submitted and approved. Exited plan mode. Resume normal operations — you may now write files, run commands, and implement the plan.",
            "exited_plan_mode": true
        })))
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
}

const EXIT_PLAN_MODE_PROMPT: &str = r#"ExitPlanMode submits your implementation plan for approval.

**When to use:**
- After you have written a complete plan to the plan file using FileWrite
- The plan should include: problem summary, context, approach, implementation steps, affected files, risks
- Do NOT use ExitPlanMode for exploration-only tasks with no plan

**How it works:**
1. Write your plan to the plan file (path given by EnterPlanMode) using FileWrite
2. Call ExitPlanMode (no arguments needed — it reads from disk)
3. Alternatively, pass the plan inline via the `plan` argument

**After approval:**
You will exit plan mode and can immediately begin implementing the plan.
Start with the first step — don't re-explain the plan unless asked."#;
