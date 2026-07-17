//! Stable, semantic policy extension points for the agent runtime.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;
use crate::model::message::{Message, ToolCall, ToolResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Create,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyPoint {
    SessionStart { mode: Option<SessionMode> },
    ModelResponse,
    ToolBeforeInvoke { matcher: Option<String> },
    ToolAfterInvoke { matcher: Option<String> },
    TurnBeforeFinish,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "point", rename_all = "snake_case")]
pub enum PolicyContext {
    SessionStart { session_id: String, mode: SessionMode, message_count: usize },
    ModelResponse { session_id: String, turn_id: u64, iteration: usize, message: Message },
    ToolBeforeInvoke { session_id: String, turn_id: u64, call: ToolCall },
    ToolAfterInvoke { session_id: String, turn_id: u64, call: ToolCall, result: ToolResult },
    TurnBeforeFinish { session_id: String, turn_id: u64, message: Message },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PolicyDecision {
    Continue,
    Reject { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyOutcome {
    #[serde(flatten)]
    pub decision: PolicyDecision,
    #[serde(default)]
    pub feedback: Vec<String>,
}

impl PolicyOutcome {
    pub fn continue_() -> Self {
        Self { decision: PolicyDecision::Continue, feedback: Vec::new() }
    }
}

#[async_trait]
pub trait Policy: Send + Sync {
    fn name(&self) -> &str;
    async fn evaluate(&self, context: &PolicyContext) -> Result<PolicyOutcome, AgentError>;
}

#[derive(Clone)]
pub struct PolicyEntry {
    pub point: PolicyPoint,
    pub policy: Arc<dyn Policy>,
}

impl std::fmt::Debug for PolicyEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyEntry")
            .field("point", &self.point)
            .field("policy", &self.policy.name())
            .finish()
    }
}

#[derive(Clone, Default)]
pub struct PolicyRegistry {
    entries: Vec<PolicyEntry>,
}

impl std::fmt::Debug for PolicyRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyRegistry").field("count", &self.entries.len()).finish()
    }
}

impl PolicyRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, entry: PolicyEntry) {
        self.entries.push(entry);
    }

    pub fn session_start(&self, mode: SessionMode) -> Vec<Arc<dyn Policy>> {
        self.entries
            .iter()
            .filter_map(|entry| match entry.point {
                PolicyPoint::SessionStart { mode: configured }
                    if configured.is_none_or(|value| value == mode) =>
                {
                    Some(entry.policy.clone())
                }
                _ => None,
            })
            .collect()
    }

    pub fn model_response(&self) -> Vec<Arc<dyn Policy>> {
        self.by_point(|point| matches!(point, PolicyPoint::ModelResponse))
    }

    pub fn turn_before_finish(&self) -> Vec<Arc<dyn Policy>> {
        self.by_point(|point| matches!(point, PolicyPoint::TurnBeforeFinish))
    }

    pub fn tool_before(&self, name: &str) -> Vec<Arc<dyn Policy>> {
        self.tool_policies(name, false)
    }

    pub fn tool_after(&self, name: &str) -> Vec<Arc<dyn Policy>> {
        self.tool_policies(name, true)
    }

    fn by_point(&self, predicate: impl Fn(&PolicyPoint) -> bool) -> Vec<Arc<dyn Policy>> {
        self.entries
            .iter()
            .filter(|entry| predicate(&entry.point))
            .map(|entry| entry.policy.clone())
            .collect()
    }

    fn tool_policies(&self, name: &str, after: bool) -> Vec<Arc<dyn Policy>> {
        self.entries
            .iter()
            .filter_map(|entry| {
                let matcher = match &entry.point {
                    PolicyPoint::ToolBeforeInvoke { matcher } if !after => matcher,
                    PolicyPoint::ToolAfterInvoke { matcher } if after => matcher,
                    _ => return None,
                };
                matcher
                    .as_ref()
                    .is_none_or(|pattern| {
                        glob::Pattern::new(pattern).is_ok_and(|glob| glob.matches(name))
                    })
                    .then(|| entry.policy.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Named(&'static str);
    #[async_trait]
    impl Policy for Named {
        fn name(&self) -> &str {
            self.0
        }
        async fn evaluate(&self, _: &PolicyContext) -> Result<PolicyOutcome, AgentError> {
            Ok(PolicyOutcome::continue_())
        }
    }

    #[test]
    fn semantic_points_filter_without_exposing_runtime_phases() {
        let mut registry = PolicyRegistry::new();
        registry.register(PolicyEntry {
            point: PolicyPoint::ToolBeforeInvoke { matcher: Some("Bash*".into()) },
            policy: Arc::new(Named("specific")),
        });
        registry.register(PolicyEntry {
            point: PolicyPoint::ToolBeforeInvoke { matcher: None },
            policy: Arc::new(Named("all")),
        });
        let names: Vec<_> = registry
            .tool_before("Bash")
            .into_iter()
            .map(|policy| policy.name().to_string())
            .collect();
        assert_eq!(names, ["specific", "all"]);
        assert_eq!(registry.tool_before("Grep").len(), 1);
        assert!(registry.tool_after("Bash").is_empty());
    }

    #[test]
    fn outcome_uses_flat_command_protocol() {
        let outcome: PolicyOutcome = serde_json::from_value(serde_json::json!({
            "decision": "reject", "reason": "blocked", "feedback": ["note"]
        }))
        .unwrap();
        assert_eq!(outcome.feedback, ["note"]);
        assert_eq!(outcome.decision, PolicyDecision::Reject { reason: "blocked".into() });
    }
}
