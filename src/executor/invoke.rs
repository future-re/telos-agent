//! Tool invocation pipeline: validate → permission → approval → invoke.

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::message::{ToolCall, ToolResult};
use crate::permissions::RuleDecision;
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
    match tool.validate(&call.arguments, &context).await {
        Ok(()) => {
            // Run JSON Schema validation after the tool's custom validation so
            // both business rules and schema shape are enforced.
            if config.auto_validate_schema
                && let Err(err) = tools.validate_arguments(&call.name, &call.arguments)
            {
                return (
                    Vec::new(),
                    ToolResult {
                        tool_call_id: call.id,
                        name: call.name,
                        content: json_error_payload("validation_error", err.to_string()),
                        is_error: true,
                    },
                );
            }

            // The global permission engine wins if it has a rule for this
            // call; otherwise we ask the tool itself.
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
            let permission_names_ref: Vec<&str> =
                permission_names.iter().map(|s| s.as_str()).collect();
            let is_shell_tool = canonical_name == "Bash";
            let engine_decision = config.permission_engine.as_ref().and_then(|engine| {
                if is_shell_tool {
                    let command = call
                        .arguments
                        .get("command")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    engine.evaluate_shell_call(
                        &permission_names_ref,
                        command,
                        &call.arguments,
                        &context.cwd,
                    )
                } else {
                    engine.evaluate_call_any(&permission_names_ref, &call.arguments, &context.cwd)
                }
            });
            let permission = match engine_decision {
                Some(RuleDecision::Allow) => Ok(PermissionDecision::Allow),
                Some(RuleDecision::Deny) => {
                    Ok(PermissionDecision::Deny { reason: "denied by permission rule".into() })
                }
                Some(RuleDecision::Ask) => Ok(PermissionDecision::Ask {
                    reason: "approval required by permission rule".into(),
                }),
                None => tool.check_permission(&call.arguments, &context).await,
            };

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
                                Ok(PermissionDecision::Allow)
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
                    let invoke_result = {
                        let invoke_fut = tool.invoke(call.arguments.clone(), context);
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
                    events,
                    ToolResult {
                        tool_call_id: call.id,
                        name: call.name.clone(),
                        content: json_error_payload("permission_denied", reason),
                        is_error: true,
                    },
                ),
                Ok(PermissionDecision::Ask { reason }) => (
                    events,
                    ToolResult {
                        tool_call_id: call.id,
                        name: call.name.clone(),
                        content: json_error_payload("permission_required", reason),
                        is_error: true,
                    },
                ),
                Err(err) => (
                    events,
                    ToolResult {
                        tool_call_id: call.id,
                        name: call.name.clone(),
                        content: json_error_payload("permission_error", err.to_string()),
                        is_error: true,
                    },
                ),
            }
        }
        Err(err) => (
            Vec::new(),
            ToolResult {
                tool_call_id: call.id,
                name: call.name.clone(),
                content: json_error_payload("validation_error", err.to_string()),
                is_error: true,
            },
        ),
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
