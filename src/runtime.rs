use async_stream::try_stream;
use futures_core::stream::Stream;
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::compaction::{CompactionConfig, compact_tool_result_message};
use crate::error::AgentError;
use crate::hooks::{HookContext, HookPhase, HookRegistry};
use crate::message::{ContentBlock, Message, ToolResult};
use crate::provider::{CompletionRequest, ModelProvider, StopReason};
use crate::tool::{PermissionDecision, ToolContext, ToolRegistry};

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub system_prompt: Option<String>,
    pub max_iterations: usize,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub max_tool_result_chars: usize,
    pub hooks: Arc<HookRegistry>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: None,
            max_iterations: 8,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            env: std::env::vars().collect(),
            max_tool_result_chars: usize::MAX,
            hooks: Arc::new(HookRegistry::new()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum TurnEvent {
    TurnStarted {
        session_id: String,
        turn_id: u64,
        user_input: String,
    },
    IterationStarted {
        iteration: usize,
        message_count: usize,
    },
    ProviderRequest {
        iteration: usize,
        message_count: usize,
        tool_count: usize,
    },
    AssistantDelta {
        text: String,
    },
    User(Message),
    Assistant(Message),
    ToolCall {
        tool_call_id: String,
        name: String,
    },
    ToolResult(Message),
    CompactionStarted {
        reason: String,
    },
    CompactionCompleted {
        reason: String,
    },
    HookStarted {
        phase: &'static str,
        name: String,
    },
    HookCompleted {
        phase: &'static str,
        name: String,
        emitted_message: bool,
    },
    TurnFinished {
        stop_reason: StopReason,
        final_text: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnResult {
    pub events: Vec<TurnEvent>,
    pub final_message: Message,
    pub stop_reason: StopReason,
}

pub struct AgentSession {
    config: AgentConfig,
    session_id: String,
    next_turn_id: u64,
    messages: Vec<Message>,
}

impl AgentSession {
    pub fn new(config: AgentConfig) -> Self {
        let mut messages = Vec::new();
        if let Some(system_prompt) = config.system_prompt.as_ref() {
            messages.push(Message::system(system_prompt.clone()));
        }

        Self {
            config,
            session_id: format!("session-{}", NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)),
            next_turn_id: 1,
            messages,
        }
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn reset(&mut self) {
        self.messages.retain(|message| message.role == crate::message::Role::System);
        self.next_turn_id = 1;
    }

    pub fn run_turn_stream<'a, P: ModelProvider + 'a>(
        &'a mut self,
        provider: &'a P,
        tools: &'a ToolRegistry,
        user_input: impl Into<String> + 'a,
    ) -> impl Stream<Item = Result<TurnEvent, AgentError>> + 'a {
        try_stream! {
            let turn_id = self.next_turn_id;
            self.next_turn_id += 1;
            let user_input = user_input.into();
            let user_message = Message::user(user_input.clone());
            self.messages.push(user_message.clone());

            yield TurnEvent::TurnStarted {
                session_id: self.session_id.clone(),
                turn_id,
                user_input,
            };
            yield TurnEvent::User(user_message);

            let mut iterations = 0;
            loop {
                if iterations >= self.config.max_iterations {
                    Err(AgentError::MaxIterations(self.config.max_iterations))?;
                }
                iterations += 1;
                let tool_definitions = tools.definitions();

                yield TurnEvent::IterationStarted {
                    iteration: iterations,
                    message_count: self.messages.len(),
                };
                yield TurnEvent::ProviderRequest {
                    iteration: iterations,
                    message_count: self.messages.len(),
                    tool_count: tool_definitions.len(),
                };

                let response = provider
                    .complete(CompletionRequest {
                        system_prompt: self.config.system_prompt.clone(),
                        messages: self.messages.clone(),
                        tools: tool_definitions,
                    })
                    .await?;

                let stop_reason = response.stop_reason;
                let assistant_message = response.message;
                for block in &assistant_message.blocks {
                    if let ContentBlock::Text(text) = block {
                        yield TurnEvent::AssistantDelta {
                            text: text.text.clone(),
                        };
                    }
                }
                self.messages.push(assistant_message.clone());
                yield TurnEvent::Assistant(assistant_message.clone());

                let hook_context = HookContext {
                    session_id: self.session_id.clone(),
                    turn_id,
                    message_count: self.messages.len(),
                };

                for hook in self.config.hooks.hooks_for_phase(HookPhase::PostSampling) {
                    yield TurnEvent::HookStarted {
                        phase: "post_sampling",
                        name: hook.name().to_string(),
                    };
                    let maybe_message = hook.run(&hook_context, &assistant_message).await?;
                    let emitted = maybe_message.is_some();
                    if let Some(message) = maybe_message {
                        self.messages.push(message.clone());
                        yield TurnEvent::Assistant(message);
                    }
                    yield TurnEvent::HookCompleted {
                        phase: "post_sampling",
                        name: hook.name().to_string(),
                        emitted_message: emitted,
                    };
                }

                if stop_reason != StopReason::ToolUse {
                    for hook in self.config.hooks.hooks_for_phase(HookPhase::Stop) {
                        yield TurnEvent::HookStarted {
                            phase: "stop",
                            name: hook.name().to_string(),
                        };
                        let maybe_message = hook.run(&hook_context, &assistant_message).await?;
                        let emitted = maybe_message.is_some();
                        if let Some(message) = maybe_message {
                            self.messages.push(message.clone());
                            yield TurnEvent::Assistant(message);
                        }
                        yield TurnEvent::HookCompleted {
                            phase: "stop",
                            name: hook.name().to_string(),
                            emitted_message: emitted,
                        };
                    }

                    yield TurnEvent::TurnFinished {
                        stop_reason,
                        final_text: assistant_message.text_content(),
                    };
                    break;
                }

                let tool_calls = assistant_message.tool_calls().cloned().collect::<Vec<_>>();
                if tool_calls.is_empty() {
                    yield TurnEvent::TurnFinished {
                        stop_reason,
                        final_text: assistant_message.text_content(),
                    };
                    break;
                }

                let mut tool_results = Vec::with_capacity(tool_calls.len());
                for call in tool_calls {
                    yield TurnEvent::ToolCall {
                        tool_call_id: call.id.clone(),
                        name: call.name.clone(),
                    };

                    let result = match tools.get(&call.name) {
                        Ok(tool) => {
                            let context = ToolContext {
                                session_id: self.session_id.clone(),
                                turn_id,
                                cwd: self.config.cwd.clone(),
                                env: self.config.env.clone(),
                                messages: self.messages.clone(),
                            };

                            match tool.validate(&call.arguments, &context).await {
                                Ok(()) => match tool.check_permission(&call.arguments, &context).await {
                                    Ok(PermissionDecision::Allow) => match tool.invoke(call.arguments.clone(), context).await {
                                        Ok(output) => ToolResult {
                                            tool_call_id: call.id,
                                            name: call.name,
                                            content: output.content,
                                            is_error: false,
                                        },
                                        Err(err) => ToolResult {
                                            tool_call_id: call.id,
                                            name: call.name.clone(),
                                            content: json_error_payload("execution_error", err.to_string()),
                                            is_error: true,
                                        },
                                    },
                                    Ok(PermissionDecision::Deny { reason }) => ToolResult {
                                        tool_call_id: call.id,
                                        name: call.name.clone(),
                                        content: json_error_payload("permission_denied", reason),
                                        is_error: true,
                                    },
                                    Ok(PermissionDecision::Ask { reason }) => ToolResult {
                                        tool_call_id: call.id,
                                        name: call.name.clone(),
                                        content: json_error_payload("permission_required", reason),
                                        is_error: true,
                                    },
                                    Err(err) => ToolResult {
                                        tool_call_id: call.id,
                                        name: call.name.clone(),
                                        content: json_error_payload("permission_error", err.to_string()),
                                        is_error: true,
                                    },
                                },
                                Err(err) => ToolResult {
                                    tool_call_id: call.id,
                                    name: call.name.clone(),
                                    content: json_error_payload("validation_error", err.to_string()),
                                    is_error: true,
                                },
                            }
                        }
                        Err(err) => ToolResult {
                            tool_call_id: call.id,
                            name: call.name.clone(),
                            content: json_error_payload("tool_not_found", err.to_string()),
                            is_error: true,
                        },
                    };
                    tool_results.push(result);
                }

                let tool_message = Message::tool_results(tool_results);
                let compaction_config = CompactionConfig {
                    max_tool_result_chars: self.config.max_tool_result_chars,
                };
                let compaction = compact_tool_result_message(tool_message, &compaction_config);
                if compaction.compacted {
                    yield TurnEvent::CompactionStarted {
                        reason: "tool_result_budget".into(),
                    };
                    yield TurnEvent::CompactionCompleted {
                        reason: "tool_result_budget".into(),
                    };
                }
                self.messages.push(compaction.message.clone());
                yield TurnEvent::ToolResult(compaction.message);
            }
        }
    }

    pub async fn run_turn<P: ModelProvider>(
        &mut self,
        provider: &P,
        tools: &ToolRegistry,
        user_input: impl Into<String>,
    ) -> Result<TurnResult, AgentError> {
        let mut stream = Box::pin(self.run_turn_stream(provider, tools, user_input));
        let mut events = Vec::new();
        let mut final_message = None;
        let mut stop_reason = StopReason::EndTurn;

        while let Some(event) = stream.next().await {
            let event = event?;
            if let TurnEvent::Assistant(message) = &event {
                final_message = Some(message.clone());
            }
            if let TurnEvent::TurnFinished {
                stop_reason: reason, ..
            } = event.clone()
            {
                stop_reason = reason;
            }
            events.push(event);
        }

        Ok(TurnResult {
            final_message: final_message.unwrap_or_else(|| Message::assistant("")),
            events,
            stop_reason,
        })
    }
}

impl TurnEvent {
    pub fn message(&self) -> &Message {
        match self {
            TurnEvent::User(message)
            | TurnEvent::Assistant(message)
            | TurnEvent::ToolResult(message) => message,
            _ => panic!("event has no message"),
        }
    }

    pub fn text(&self) -> String {
        match self {
            TurnEvent::TurnStarted {
                session_id,
                turn_id,
                user_input,
            } => format!("turn_started:{}#{}:{}", session_id, turn_id, user_input),
            TurnEvent::IterationStarted {
                iteration,
                message_count,
            } => format!("iteration_started:{} messages={}", iteration, message_count),
            TurnEvent::ProviderRequest {
                iteration,
                message_count,
                tool_count,
            } => format!(
                "provider_request:{} messages={} tools={}",
                iteration, message_count, tool_count
            ),
            TurnEvent::AssistantDelta { text } => format!("assistant_delta:{text}"),
            TurnEvent::ToolCall { tool_call_id, name } => {
                format!("tool_call:{}#{}", name, tool_call_id)
            }
            TurnEvent::CompactionStarted { reason } => {
                format!("compaction_started:{reason}")
            }
            TurnEvent::CompactionCompleted { reason } => {
                format!("compaction_completed:{reason}")
            }
            TurnEvent::HookStarted { phase, name } => {
                format!("hook_started:{phase}:{name}")
            }
            TurnEvent::HookCompleted {
                phase,
                name,
                emitted_message,
            } => format!("hook_completed:{phase}:{name}:{emitted_message}"),
            TurnEvent::TurnFinished {
                stop_reason,
                final_text,
            } => format!("turn_finished:{stop_reason:?}:{final_text}"),
            _ => self
                .message()
                .blocks
                .iter()
                .map(|block| match block {
                    ContentBlock::Text(text) => text.text.clone(),
                    ContentBlock::ToolCall(call) => {
                        format!("tool_call:{}({})", call.name, call.arguments)
                    }
                    ContentBlock::ToolResult(result) => {
                        format!("tool_result:{}={}", result.name, result.content)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

fn json_error_payload(kind: &str, message: String) -> serde_json::Value {
    json!({
        "error": {
            "kind": kind,
            "message": message,
        }
    })
}
