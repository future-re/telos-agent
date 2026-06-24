//! `EnterPlanMode` tool — switch the agent into a read-only planning mode.
//!
//! When invoked, the tool instructs the model to explore, design, and document
//! an implementation plan *without* modifying any files. The model is expected
//! to write the plan to the plan file path, then call `ExitPlanMode` to submit
//! it for approval.
//!
//! Key behaviours from learn-claude-code:
//! - Stops the model from writing files (prompt-level enforcement)
//! - Provides a plan file path for the model to write to
//! - Returns detailed instructions for exploration → design → exit workflow

use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

/// Shared plan-mode state so `ExitPlanMode` knows the plan file path chosen
/// by `EnterPlanMode`.
#[derive(Debug, Clone, Default)]
pub struct PlanModeState {
    pub plan_file_path: Option<PathBuf>,
    pub active: bool,
}

pub type SharedPlanState = Arc<Mutex<PlanModeState>>;

/// `EnterPlanMode` — switch to read-only exploration/design mode.
///
/// The model must write the plan to the plan file, then call `ExitPlanMode`.
pub struct EnterPlanModeTool {
    plan_state: SharedPlanState,
}

impl EnterPlanModeTool {
    pub fn new(plan_state: SharedPlanState) -> Self {
        Self { plan_state }
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "EnterPlanMode".into(),
            description:
                "Enter read-only plan mode for complex tasks (3+ files, ambiguous requirements, restructuring). \
Explore, design, write a plan, then call ExitPlanMode to submit. Do NOT modify files while in plan mode."
                    .into(),
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(PLAN_MODE_PROMPT)
    }

    async fn invoke(
        &self,
        _arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let plan_file = context.cwd.join("plan.md");

        {
            let mut state = self.plan_state.lock().unwrap();
            state.plan_file_path = Some(plan_file.clone());
            state.active = true;
        }

        Ok(ToolOutput::text(format!(
            "Plan mode activated. Plan file: {}\n\n\
You are now in PLAN MODE. Follow these steps:\n\
1. **Explore** the codebase — use Read, Grep, Glob, WebSearch to understand the problem.\n\
2. **Design** the solution — consider alternatives, identify constraints, estimate effort.\n\
3. **Write the plan** — use FileWrite to save the full plan to `{}`.\n\
4. **Call ExitPlanMode** — when the plan is complete, call ExitPlanMode to submit it.\n\n\
While in plan mode:\n\
- DO NOT use FileEdit, Bash (write/mutate commands), or any other write tool.\n\
- You may use Read, Grep, Glob, WebSearch, WebFetch, CodeSearch freely.\n\
- The plan should include: problem summary, approach, affected files, steps, risks.",
            plan_file.display(),
            plan_file.display()
        )))
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
}

const PLAN_MODE_PROMPT: &str = r#"EnterPlanMode puts you in a read-only exploration and design mode.

**When to use:** Multi-file changes (3+ files), new features, cross-module refactoring, ambiguous requirements, or high-impact restructuring. Skip for single-file trivial fixes or when the user shows exactly what to do.

**Workflow:**
1. Call EnterPlanMode → get plan file path
2. Explore codebase with Read, Grep, Glob, WebSearch
3. Write plan to plan file using Write (cover: Problem, Context, Approach, Implementation Steps, Affected Files, Risks)
4. Call ExitPlanMode to submit for approval

Do NOT use file edit or shell while in plan mode. The plan should be detailed enough that another agent could follow it."#;
