//! Tool invocation pipeline: validate → permission → approval → invoke.

use crate::config::AgentConfig;
use crate::diagnostics::{ToolFailureKind, sanitized_event_for_failure};
use crate::error::AgentError;
use crate::message::{ToolCall, ToolResult};
use crate::permissions::{RuleDecision, ShellKind};
use crate::tool::{PermissionDecision, ToolContext, ToolRegistry};
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{Instrument, debug, error, info_span, warn};

use super::types::ToolExecutionEvent;

pub(crate) async fn invoke_existing_tool(
    mut call: ToolCall,
    tool: Arc<dyn crate::tool::Tool>,
    context: ToolContext,
    config: &AgentConfig,
    tools: &ToolRegistry,
) -> (Vec<ToolExecutionEvent>, ToolResult) {
    let canonical_name = tool.definition().name;
    let mut permission_names = vec![call.name.clone()];
    if canonical_name != call.name {
        permission_names.push(canonical_name.clone());
    }
    for alias in tool.aliases() {
        if !permission_names.iter().any(|n| n == alias) {
            permission_names.push((*alias).to_string());
        }
    }

    match validate_tool_call(&call, &tool, &context, config, tools).await {
        Ok(()) => {
            let permission =
                evaluate_permission(&call, &tool, &context, config, &permission_names).await;

            let mut events = Vec::new();
            let permission = match permission {
                Ok(PermissionDecision::Ask { reason }) => {
                    // If an approval handler is configured, suspend the call and
                    // ask for explicit human approval. Otherwise fall through to
                    // the legacy error-result behaviour.
                    if let Some(handler) = &config.approval_handler {
                        events.push(ToolExecutionEvent::ApprovalRequested {
                            tool_call_id: call.id.clone(),
                            name: call.name.clone(),
                            reason: reason.clone(),
                        });
                        let request = crate::approval::ApprovalRequest {
                            tool_name: canonical_name.clone(),
                            invocation_names: permission_names.clone(),
                            arguments: call.arguments.clone(),
                            cwd: context.cwd.clone(),
                            messages: context.messages.clone(),
                            reason: reason.clone(),
                        };
                        let decision = handler.ask(request).await;
                        events.push(ToolExecutionEvent::ApprovalResolved {
                            tool_call_id: call.id.clone(),
                            name: call.name.clone(),
                            decision: format!("{decision:?}"),
                        });
                        match decision {
                            crate::approval::ApprovalDecision::Allow => {
                                Ok(PermissionDecision::Allow)
                            }
                            crate::approval::ApprovalDecision::Deny { reason } => {
                                Ok(PermissionDecision::Deny { reason })
                            }
                            crate::approval::ApprovalDecision::Modify { arguments } => {
                                call.arguments = arguments;
                                match validate_tool_call(&call, &tool, &context, config, tools)
                                    .await
                                {
                                    Ok(()) => {
                                        match evaluate_permission(
                                            &call,
                                            &tool,
                                            &context,
                                            config,
                                            &permission_names,
                                        )
                                        .await
                                        {
                                            Ok(PermissionDecision::Deny { reason }) => {
                                                Ok(PermissionDecision::Deny { reason })
                                            }
                                            Err(err) => Err(err),
                                            Ok(PermissionDecision::Ask { reason })
                                                if canonical_name == "Bash" =>
                                            {
                                                Ok(PermissionDecision::Ask {
                                                    reason: format!(
                                                        "modified shell command requires separate approval: {reason}"
                                                    ),
                                                })
                                            }
                                            Ok(PermissionDecision::Ask { .. })
                                            | Ok(PermissionDecision::Allow) => {
                                                Ok(PermissionDecision::Allow)
                                            }
                                        }
                                    }
                                    Err(err) => Err(err),
                                }
                            }
                        }
                    } else {
                        Ok(PermissionDecision::Ask { reason })
                    }
                }
                other => other,
            };

            match permission {
                Ok(PermissionDecision::Allow) => {
                    let invoke_span =
                        info_span!("tool_execution", tool = %call.name, tool_call_id = %call.id);
                    let tool_name = call.name.clone();
                    let invoke_context = context.clone();
                    let invoke_result = {
                        let invoke_fut = tool.invoke(call.arguments.clone(), invoke_context);
                        // A timeout of 0ms is treated as "no timeout" to avoid
                        // immediately failing every tool call.
                        let timeout = config.tool_timeout_ms.filter(|&ms| ms > 0);
                        async move {
                            if let Some(ms) = timeout {
                                match tokio::time::timeout(
                                    std::time::Duration::from_millis(ms),
                                    invoke_fut,
                                )
                                .await
                                {
                                    Ok(result) => result,
                                    Err(_elapsed) => {
                                        warn!("tool timed out after {}ms", ms);
                                        Err(AgentError::ToolExecution {
                                            tool: tool_name,
                                            message: format!("timed out after {}ms", ms),
                                        })
                                    }
                                }
                            } else {
                                invoke_fut.await
                            }
                        }
                        .instrument(invoke_span)
                        .await
                    };
                    match invoke_result {
                        Ok(output) => {
                            debug!("tool succeeded");
                            (
                                events,
                                ToolResult {
                                    tool_call_id: call.id,
                                    name: call.name,
                                    content: output.content,
                                    is_error: false,
                                },
                            )
                        }
                        Err(err) => {
                            error!(error = %err, "tool failed");
                            record_tool_failure(
                                config,
                                &context,
                                &call,
                                ToolFailureKind::ExecutionError,
                                &err.to_string(),
                            )
                            .await;
                            (
                                events,
                                ToolResult {
                                    tool_call_id: call.id,
                                    name: call.name.clone(),
                                    content: json_error_payload("execution_error", err.to_string()),
                                    is_error: true,
                                },
                            )
                        }
                    }
                }
                Ok(PermissionDecision::Deny { reason }) => (
                    {
                        record_tool_failure(
                            config,
                            &context,
                            &call,
                            ToolFailureKind::PermissionDenied,
                            &reason,
                        )
                        .await;
                        events
                    },
                    ToolResult {
                        tool_call_id: call.id,
                        name: call.name.clone(),
                        content: json_error_payload("permission_denied", reason),
                        is_error: true,
                    },
                ),
                Ok(PermissionDecision::Ask { reason }) => (
                    {
                        record_tool_failure(
                            config,
                            &context,
                            &call,
                            ToolFailureKind::PermissionRequired,
                            &reason,
                        )
                        .await;
                        events
                    },
                    ToolResult {
                        tool_call_id: call.id,
                        name: call.name.clone(),
                        content: json_error_payload("permission_required", reason),
                        is_error: true,
                    },
                ),
                Err(err) => (
                    {
                        record_tool_failure(
                            config,
                            &context,
                            &call,
                            ToolFailureKind::PermissionError,
                            &err.to_string(),
                        )
                        .await;
                        events
                    },
                    ToolResult {
                        tool_call_id: call.id,
                        name: call.name.clone(),
                        content: json_error_payload("permission_error", err.to_string()),
                        is_error: true,
                    },
                ),
            }
        }
        Err(err) => {
            record_tool_failure(
                config,
                &context,
                &call,
                ToolFailureKind::ValidationError,
                &err.to_string(),
            )
            .await;
            (
                Vec::new(),
                ToolResult {
                    tool_call_id: call.id,
                    name: call.name.clone(),
                    content: json_error_payload("validation_error", err.to_string()),
                    is_error: true,
                },
            )
        }
    }
}

pub(crate) async fn record_tool_failure(
    config: &AgentConfig,
    context: &ToolContext,
    call: &ToolCall,
    failure_kind: ToolFailureKind,
    error: &str,
) {
    let Some(sink) = &config.tool_diagnostics else {
        return;
    };
    let event = sanitized_event_for_failure(
        &context.session_id,
        context.turn_id,
        &call.id,
        &call.name,
        failure_kind,
        &call.arguments,
        error,
        &context.cwd,
        &context.env,
    );
    if let Err(err) = sink.record(event).await {
        warn!(error = %err, "failed to record tool diagnostics");
    }
}

async fn validate_tool_call(
    call: &ToolCall,
    tool: &Arc<dyn crate::tool::Tool>,
    context: &ToolContext,
    config: &AgentConfig,
    tools: &ToolRegistry,
) -> Result<(), AgentError> {
    tool.validate(&call.arguments, context).await?;
    // Run JSON Schema validation after the tool's custom validation so both
    // business rules and schema shape are enforced.
    if config.auto_validate_schema {
        tools.validate_arguments(&call.name, &call.arguments)?;
    }
    Ok(())
}

async fn evaluate_permission(
    call: &ToolCall,
    tool: &Arc<dyn crate::tool::Tool>,
    context: &ToolContext,
    config: &AgentConfig,
    permission_names: &[String],
) -> Result<PermissionDecision, AgentError> {
    // The global permission engine wins if it has a rule for this call;
    // otherwise we ask the tool itself.
    let canonical_name = tool.definition().name;
    let permission_names_ref: Vec<&str> = permission_names.iter().map(|s| s.as_str()).collect();
    let shell_kind = match canonical_name.as_str() {
        "Bash" => Some(ShellKind::Bash),
        "PowerShell" => Some(ShellKind::PowerShell),
        _ => None,
    };
    let engine_decision = config.permission_engine.as_ref().and_then(|engine| {
        if let Some(shell_kind) = shell_kind {
            let command =
                call.arguments.get("command").and_then(|value| value.as_str()).unwrap_or("");
            engine.evaluate_shell_call(
                shell_kind,
                &permission_names_ref,
                command,
                &call.arguments,
                &context.cwd,
            )
        } else {
            engine.evaluate_call_any(&permission_names_ref, &call.arguments, &context.cwd)
        }
    });

    match engine_decision {
        Some(RuleDecision::Allow) => Ok(PermissionDecision::Allow),
        Some(RuleDecision::Deny) => {
            Ok(PermissionDecision::Deny { reason: "denied by permission rule".into() })
        }
        Some(RuleDecision::Ask) => {
            Ok(PermissionDecision::Ask { reason: "approval required by permission rule".into() })
        }
        None => tool.check_permission(&call.arguments, context).await,
    }
}

pub(crate) fn json_error_payload(kind: &str, message: String) -> Value {
    json!({
        "error": {
            "kind": kind,
            "message": message,
        }
    })
}
