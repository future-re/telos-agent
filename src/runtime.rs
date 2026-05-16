use async_stream::try_stream;
use futures_core::stream::Stream;
use futures_util::StreamExt;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::compaction::{CompactionConfig, compact_tool_result_message};
use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::executor::{ToolExecutionEvent, ToolExecutionStreamItem, execute_tool_calls_stream};
use crate::hooks::{HookContext, HookPhase};
use crate::message::{ContentBlock, Message, Role, TextBlock};
use crate::provider::{CompletionRequest, ModelProvider, ProviderEvent, StopReason, TokenUsage};
use crate::storage::Storage;
use crate::tool::ToolRegistry;

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

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
    ProviderUsage {
        input_tokens: usize,
        output_tokens: usize,
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
    ToolProgress {
        tool_call_id: Option<String>,
        name: String,
        message: String,
        data: Option<serde_json::Value>,
    },
    ToolCompleted {
        tool_call_id: String,
        name: String,
        is_error: bool,
    },
    ToolResult(Message),
    CompactionStarted {
        reason: String,
    },
    CompactionCompleted {
        reason: String,
    },
    TokenBudgetExceeded {
        used_tokens: usize,
        max_tokens: usize,
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
            session_id: format!(
                "session-{}",
                NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
            ),
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
        self.messages
            .retain(|message| message.role == crate::message::Role::System);
        self.next_turn_id = 1;
    }

    pub async fn save(&self) -> Result<(), AgentError> {
        if let Some(storage) = &self.config.storage {
            storage
                .save_snapshot(&self.session_id, &self.messages)
                .await?;
        }
        Ok(())
    }

    pub async fn resume(
        session_id: impl Into<String>,
        mut config: AgentConfig,
        storage: Arc<dyn Storage>,
    ) -> Result<Self, AgentError> {
        let session_id = session_id.into();
        let mut messages = storage.load(&session_id).await?;
        if messages.is_empty() {
            if let Some(system_prompt) = config.system_prompt.as_ref() {
                messages.push(Message::system(system_prompt.clone()));
            }
        } else {
            // Ensure the loaded system prompt matches config
            let loaded_system = messages
                .first()
                .filter(|m| m.role == crate::message::Role::System)
                .map(|m| m.text_content());
            if let Some(config_system) = &config.system_prompt {
                if loaded_system.as_deref() != Some(config_system.as_str()) {
                    // Replace system prompt if config differs
                    if messages.first().map(|m| m.role) == Some(crate::message::Role::System) {
                        messages[0] = Message::system(config_system.clone());
                    } else {
                        messages.insert(0, Message::system(config_system.clone()));
                    }
                }
            }
        }
        config.storage = Some(storage);
        Ok(Self {
            config,
            session_id,
            next_turn_id: 1,
            messages,
        })
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

                if let Some(budget) = self.config.token_budget {
                    let estimated_tokens = estimate_message_tokens(&self.messages, provider);
                    if estimated_tokens > budget.max_tokens {
                        yield TurnEvent::TokenBudgetExceeded {
                            used_tokens: estimated_tokens,
                            max_tokens: budget.max_tokens,
                        };
                        yield TurnEvent::TurnFinished {
                            stop_reason: StopReason::EndTurn,
                            final_text: String::new(),
                        };
                        break;
                    }
                    if estimated_tokens >= budget.compact_at_tokens {
                        if let Some(compaction) = self.config.compaction.clone() {
                            if compaction.compact(&mut self.messages, provider).await? {
                                yield TurnEvent::CompactionStarted {
                                    reason: "token_budget".into(),
                                };
                                yield TurnEvent::CompactionCompleted {
                                    reason: "token_budget".into(),
                                };
                            }
                        }
                    }
                }

                if let Some(compaction) = self.config.compaction.clone() {
                    if compaction.compact(&mut self.messages, provider).await? {
                        yield TurnEvent::CompactionStarted {
                            reason: "char_budget".into(),
                        };
                        yield TurnEvent::CompactionCompleted {
                            reason: "char_budget".into(),
                        };
                    }
                }

                let (assistant_message, stop_reason, usage) = {
                    let request = CompletionRequest {
                        system_prompt: self.config.system_prompt.clone(),
                        messages: self.messages.clone(),
                        tools: tool_definitions,
                    };
                    let mut stream = Box::pin(provider.stream_complete(request));
                    let mut blocks = Vec::new();
                    let mut stop_reason = StopReason::EndTurn;
                    let mut usage = None;
                    while let Some(event) = stream.next().await {
                        match event? {
                            ProviderEvent::MessageStart => {}
                            ProviderEvent::TextDelta(text) => {
                                yield TurnEvent::AssistantDelta {
                                    text: text.clone(),
                                };
                                blocks.push(ContentBlock::Text(TextBlock { text }));
                            }
                            ProviderEvent::ToolCall(call) => {
                                blocks.push(ContentBlock::ToolCall(call));
                            }
                            ProviderEvent::MessageStop {
                                stop_reason: reason,
                                usage: event_usage,
                            } => {
                                stop_reason = reason;
                                usage = event_usage;
                            }
                        }
                    }
                    (
                        Message {
                            role: Role::Assistant,
                            blocks,
                        },
                        stop_reason,
                        usage,
                    )
                };
                if let Some(TokenUsage { input_tokens, output_tokens }) = usage {
                    yield TurnEvent::ProviderUsage {
                        input_tokens,
                        output_tokens,
                    };
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

                let mut execution = Box::pin(execute_tool_calls_stream(
                    tool_calls,
                    tools,
                    &self.config,
                    &self.session_id,
                    turn_id,
                    self.messages.clone(),
                ));
                let mut tool_results = Vec::new();
                while let Some(item) = execution.next().await {
                    match item {
                        ToolExecutionStreamItem::Event(event) => {
                            match event {
                                ToolExecutionEvent::ToolStarted { tool_call_id, name } => {
                                    yield TurnEvent::ToolCall { tool_call_id, name };
                                }
                                ToolExecutionEvent::ToolProgress {
                                    tool_call_id,
                                    name,
                                    message,
                                    data,
                                } => {
                                    yield TurnEvent::ToolProgress {
                                        tool_call_id,
                                        name,
                                        message,
                                        data,
                                    };
                                }
                                ToolExecutionEvent::ToolCompleted {
                                    tool_call_id,
                                    name,
                                    is_error,
                                } => {
                                    yield TurnEvent::ToolCompleted {
                                        tool_call_id,
                                        name,
                                        is_error,
                                    };
                                }
                            }
                        }
                        ToolExecutionStreamItem::Result(result) => {
                            tool_results.push(result);
                        }
                    }
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
        let (events, final_message, stop_reason) = {
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
                    stop_reason: reason,
                    ..
                } = event.clone()
                {
                    stop_reason = reason;
                }
                events.push(event);
            }
            (events, final_message, stop_reason)
        };

        self.save().await?;

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
            TurnEvent::ProviderUsage {
                input_tokens,
                output_tokens,
            } => format!("provider_usage:input={input_tokens} output={output_tokens}"),
            TurnEvent::AssistantDelta { text } => format!("assistant_delta:{text}"),
            TurnEvent::ToolCall { tool_call_id, name } => {
                format!("tool_call:{}#{}", name, tool_call_id)
            }
            TurnEvent::ToolProgress {
                tool_call_id,
                name,
                message,
                ..
            } => format!(
                "tool_progress:{}#{}:{}",
                name,
                tool_call_id.as_deref().unwrap_or("unknown"),
                message
            ),
            TurnEvent::ToolCompleted {
                tool_call_id,
                name,
                is_error,
            } => format!(
                "tool_completed:{}#{} error={}",
                name, tool_call_id, is_error
            ),
            TurnEvent::CompactionStarted { reason } => {
                format!("compaction_started:{reason}")
            }
            TurnEvent::CompactionCompleted { reason } => {
                format!("compaction_completed:{reason}")
            }
            TurnEvent::TokenBudgetExceeded {
                used_tokens,
                max_tokens,
            } => format!("token_budget_exceeded:{used_tokens}/{max_tokens}"),
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

fn estimate_message_tokens(messages: &[Message], provider: &dyn ModelProvider) -> usize {
    messages
        .iter()
        .flat_map(|message| message.blocks.iter())
        .map(|block| match block {
            ContentBlock::Text(text) => provider.estimate_tokens(&text.text),
            ContentBlock::ToolCall(call) => {
                provider.estimate_tokens(&call.name)
                    + provider.estimate_tokens(&call.arguments.to_string())
            }
            ContentBlock::ToolResult(result) => {
                provider.estimate_tokens(&result.content.to_string())
            }
        })
        .sum()
}
