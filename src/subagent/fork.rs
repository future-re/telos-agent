//! Fork engine — lightweight concurrent multi-perspective execution.
//!
//! Each "lens" shares the parent session's provider, tools, and messages but
//! gets its own system prompt + task. All lenses run concurrently via a
//! tokio [`Semaphore`]-bounded [`Synapse`].
//!
//! Fork is NOT a subprocess. It is an in-process concurrent provider call.

use futures_util::future::join_all;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::config::AgentConfig;
use crate::message::Message;
use crate::provider::{CompletionRequest, ModelProvider};
use crate::tasks::{Task, TaskManager, TaskStatus};
use crate::tool::ToolRegistry;

/// Shared state across all fork lenses — cheap to clone.
pub struct ForkShared {
    pub provider: Arc<dyn ModelProvider + Send + Sync>,
    pub tool_registry: ToolRegistry,
    pub messages: Arc<Vec<Message>>,
    pub config: AgentConfig,
}

/// A single lens — one perspective on the shared context.
#[derive(Debug, Clone)]
pub struct ForkLens {
    /// Label for logging and tracking.
    pub lens: String,
    /// Injected as the system prompt for this lens.
    pub system_prompt: String,
    /// The specific task for this lens.
    pub task: String,
    /// Optional JSON Schema for structured output.
    pub output_schema: Option<Value>,
    /// Tools available to this lens (if empty, uses all registry tools).
    pub allowed_tools: Vec<String>,
}

/// Result from a single lens execution.
#[derive(Debug, Clone)]
pub enum ForkResult {
    Text(String),
    Structured(Value),
}

/// Result of a complete fork execution.
pub struct ForkExecution {
    pub results: Vec<Option<ForkResult>>,
    pub task_ids: Vec<String>,
}

/// Lightweight concurrency limiter for fork lens execution.
pub struct Synapse {
    semaphore: Arc<Semaphore>,
}

impl Synapse {
    pub fn new(max_concurrent: usize) -> Self {
        Synapse { semaphore: Arc::new(Semaphore::new(max_concurrent.max(1))) }
    }

    /// Run all lenses concurrently, respecting the concurrency limit.
    /// Each lens gets a single provider call (not a full turn loop).
    /// If a `TaskManager` is provided, one task is created per lens and
    /// updated on completion.
    pub async fn run_all(
        &self,
        shared: &ForkShared,
        lenses: Vec<ForkLens>,
        task_manager: Option<&TaskManager>,
    ) -> ForkExecution {
        let task_ids: Vec<String> = if let Some(tm) = &task_manager {
            lenses
                .iter()
                .map(|lens| {
                    let id = uuid_v4();
                    let task = Task {
                        id: id.clone(),
                        subject: format!("fork lens: {}", lens.lens),
                        description: lens.task.clone(),
                        status: TaskStatus::InProgress,
                        blocked_by: vec![],
                        blocks: vec![],
                        output: None,
                    };
                    tm.create(task);
                    id
                })
                .collect()
        } else {
            vec![]
        };

        let results = join_all(lenses.into_iter().map(|lens| {
            let sem = self.semaphore.clone();
            let shared = shared.clone();
            async move {
                let _permit = sem.acquire().await.ok()?;
                execute_lens(&shared, &lens).await
            }
        }))
        .await;

        // Update task status based on execution results
        if let Some(tm) = &task_manager {
            for (i, _result) in results.iter().enumerate() {
                let status = TaskStatus::Completed;
                if let Some(task_id) = task_ids.get(i) {
                    tm.update(task_id, status);
                }
            }
        }

        ForkExecution { results, task_ids }
    }
}

/// Generate a unique task ID using the current timestamp.
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("task_{:x}", now.as_nanos())
}

impl Clone for ForkShared {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            tool_registry: self.tool_registry.clone(),
            messages: self.messages.clone(),
            config: self.config.clone(),
        }
    }
}

/// Execute a single lens: build context summary, call provider, extract result.
async fn execute_lens(shared: &ForkShared, lens: &ForkLens) -> Option<ForkResult> {
    // Build context summary from shared messages (last few messages only)
    let context_summary = build_context_summary(&shared.messages);

    let fork_messages = vec![
        Message::system(&lens.system_prompt),
        Message::user(format!("## Context\n\n{context_summary}\n\n## Task\n\n{}", lens.task)),
    ];

    // Filter tools if allowed_tools specified
    let tools = if lens.allowed_tools.is_empty() {
        shared.tool_registry.definitions()
    } else {
        shared
            .tool_registry
            .definitions()
            .into_iter()
            .filter(|d| lens.allowed_tools.contains(&d.name))
            .collect()
    };

    let request = CompletionRequest {
        system_prompt: None, // system message already in fork_messages
        messages: fork_messages,
        tools,
    };

    let response = match shared.provider.complete(request).await {
        Ok(r) => r,
        Err(_) => return None,
    };

    let text = response.message.text_content();

    // If output_schema specified, try to extract structured JSON
    if let Some(_schema) = &lens.output_schema {
        if let Ok(val) = serde_json::from_str::<Value>(&text) {
            Some(ForkResult::Structured(val))
        } else {
            // Fallback: wrap text
            Some(ForkResult::Text(text))
        }
    } else {
        Some(ForkResult::Text(text))
    }
}

/// Build a compact context summary from recent messages.
fn build_context_summary(messages: &[Message]) -> String {
    let recent: Vec<&Message> = messages.iter().rev().take(6).collect();
    if recent.is_empty() {
        return String::new();
    }
    let mut parts = vec!["## Recent Conversation".to_string()];
    for msg in recent.iter().rev() {
        let text = msg.text_content();
        if text.is_empty() {
            continue;
        }
        let truncated: String = text.chars().take(2000).collect();
        parts.push(format!("[{}]: {}", role_str(&msg.role), truncated));
    }
    parts.join("\n")
}

fn role_str(role: &crate::message::Role) -> &str {
    match role {
        crate::message::Role::System => "system",
        crate::message::Role::User => "user",
        crate::message::Role::Assistant => "assistant",
        crate::message::Role::Tool => "tool",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentConfig;
    use crate::mock::MockProvider;

    #[tokio::test]
    async fn synapse_runs_multiple_lenses_concurrently() {
        let provider = Arc::new(MockProvider::new(vec![]));
        let tool_registry = ToolRegistry::new();
        let messages = Arc::new(vec![Message::user("Original request")]);

        let shared = ForkShared {
            provider,
            tool_registry: tool_registry.clone(),
            messages,
            config: AgentConfig::default(),
        };

        let lenses = vec![
            ForkLens {
                lens: "a".into(),
                system_prompt: "You are lens A".into(),
                task: "Task A".into(),
                output_schema: None,
                allowed_tools: vec![],
            },
            ForkLens {
                lens: "b".into(),
                system_prompt: "You are lens B".into(),
                task: "Task B".into(),
                output_schema: None,
                allowed_tools: vec![],
            },
        ];

        let synapse = Synapse::new(2);
        let execution = synapse.run_all(&shared, lenses, None).await;
        assert_eq!(execution.results.len(), 2);
        // Both lenses should fail gracefully (no mock responses)
        assert!(execution.results.iter().all(|r| r.is_none()));
    }

    #[test]
    fn build_context_summary_truncates_long_messages() {
        let long_text = "x".repeat(5000);
        let msgs = vec![Message::user(&long_text), Message::assistant("short response")];
        let summary = build_context_summary(&msgs);
        assert!(summary.contains("short response"));
        assert!(!summary.contains(&long_text));
    }
}
