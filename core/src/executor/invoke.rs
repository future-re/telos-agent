//! Tool invocation pipeline: validate → permission → invoke.

use crate::config::AgentConfig;
use crate::diagnostics::{ToolFailureKind, sanitized_event_for_failure};
use crate::error::AgentError;
use crate::message::{ToolCall, ToolResult};
use crate::permissions::{RuleDecision, ShellKind};
use crate::tool::{PermissionDecision, ToolContext, ToolRegistry};
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{Instrument, error, info_span, warn};

use super::types::ToolExecutionEvent;

/// Tool invocation pipeline:
///   1. Validate arguments
///   2. Resolve permission (Ask → approval handler if configured)
///   3. Invoke or return error
pub(crate) async fn invoke_tool(
    mut call: ToolCall,
    tool: Arc<dyn crate::tool::Tool>,
    context: ToolContext,
    config: &AgentConfig,
    tools: &ToolRegistry,
) -> (Vec<ToolExecutionEvent>, ToolResult) {
    // 1. Validate
    if let Err(err) = validate_call(&call, &tool, &context, config, tools).await {
        record_tool_failure(
            config,
            &context,
            &call,
            ToolFailureKind::ValidationError,
            &err.to_string(),
        )
        .await;
        return (Vec::new(), error_result(&call, "validation_error", err.to_string()));
    }

    // 2. Permission + approval loop
    let tool_name = tool.definition().name.clone();
    let mut events = Vec::new();
    let permission = loop {
        let decision = evaluate_permission(&call, &tool, &context, config, &tool_name).await;

        let Ok(PermissionDecision::Ask { ref reason }) = decision else {
            break decision;
        };
        let Some(handler) = &config.approval_handler else {
            break decision;
        };

        events.push(ToolExecutionEvent::ApprovalRequested {
            tool_call_id: call.id.clone(),
            name: call.name.clone(),
            reason: reason.clone(),
        });
        let request = crate::approval::ApprovalRequest {
            tool_call_id: call.id.clone(),
            tool_name: tool_name.to_string(),
            arguments: call.arguments.clone(),
            cwd: context.cwd.clone(),
            messages: context.messages.clone(),
            reason: reason.clone(),
        };
        let approval = handler.ask(request).await;
        events.push(ToolExecutionEvent::ApprovalResolved {
            tool_call_id: call.id.clone(),
            name: call.name.clone(),
            decision: format!("{approval:?}"),
        });

        match approval {
            crate::approval::ApprovalDecision::Allow => break Ok(PermissionDecision::Allow),
            crate::approval::ApprovalDecision::Deny { reason } => {
                break Ok(PermissionDecision::Deny { reason });
            }
            crate::approval::ApprovalDecision::Modify { arguments } => {
                call.arguments = arguments;
                if let Err(err) = validate_call(&call, &tool, &context, config, tools).await {
                    break Err(err);
                }
                if requires_fresh_approval(&tool_name) {
                    continue;
                }
                break Ok(PermissionDecision::Allow);
            }
        }
    };

    // 3. Invoke or return error
    match permission {
        Ok(PermissionDecision::Allow) => {
            match run_tool(&call, &tool, context.clone(), config).await {
                Ok(output) => (
                    events,
                    ToolResult {
                        tool_call_id: call.id.clone(),
                        name: call.name.clone(),
                        content: output.content,
                        is_error: false,
                    },
                ),
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
                    (events, error_result(&call, "execution_error", err.to_string()))
                }
            }
        }
        Ok(PermissionDecision::Deny { reason }) => {
            record_tool_failure(
                config,
                &context,
                &call,
                ToolFailureKind::PermissionDenied,
                &reason,
            )
            .await;
            (events, error_result(&call, "permission_denied", reason))
        }
        Ok(PermissionDecision::Ask { reason }) => {
            record_tool_failure(
                config,
                &context,
                &call,
                ToolFailureKind::PermissionRequired,
                &reason,
            )
            .await;
            (events, error_result(&call, "permission_required", reason))
        }
        Err(err) => {
            let msg = err.to_string();
            record_tool_failure(
                config,
                &context,
                &call,
                ToolFailureKind::PermissionError,
                &msg,
            )
            .await;
            (events, error_result(&call, "permission_error", msg))
        }
    }
}

// ── Pipeline helpers ────────────────────────────────────────────────────────

fn error_result(call: &ToolCall, kind: &str, message: String) -> ToolResult {
    ToolResult {
        tool_call_id: call.id.clone(),
        name: call.name.clone(),
        content: json_error_payload(kind, message),
        is_error: true,
    }
}

fn requires_fresh_approval(tool_name: &str) -> bool {
    matches!(tool_name, "Bash" | "PowerShell")
}

async fn validate_call(
    call: &ToolCall,
    tool: &Arc<dyn crate::tool::Tool>,
    context: &ToolContext,
    config: &AgentConfig,
    tools: &ToolRegistry,
) -> Result<(), AgentError> {
    tool.validate(&call.arguments, context).await?;
    if config.auto_validate_schema {
        tools.validate_arguments(&call.name, &call.arguments)?;
    }
    Ok(())
}

async fn run_tool(
    call: &ToolCall,
    tool: &Arc<dyn crate::tool::Tool>,
    context: ToolContext,
    config: &AgentConfig,
) -> Result<crate::tool::ToolOutput, AgentError> {
    let invoke_span =
        info_span!("tool_execution", tool = %call.name, tool_call_id = %call.id);
    let tool_name = call.name.clone();
    let invoke_fut = tool.invoke(call.arguments.clone(), context);
    let timeout = config.tool_timeout_ms.filter(|&ms| ms > 0);
    async move {
        if let Some(ms) = timeout {
            match tokio::time::timeout(std::time::Duration::from_millis(ms), invoke_fut)
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
}

// ── Public helpers — used by sync.rs / stream.rs ────────────────────────────

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

async fn evaluate_permission(
    call: &ToolCall,
    tool: &Arc<dyn crate::tool::Tool>,
    context: &ToolContext,
    config: &AgentConfig,
    tool_name: &str,
) -> Result<PermissionDecision, AgentError> {
    let permission_names = [tool_name];
    let shell_kind = match tool_name {
        "Bash" => Some(ShellKind::Bash),
        "PowerShell" => Some(ShellKind::PowerShell),
        _ => None,
    };
    let engine_decision = config.permission_engine.as_ref().and_then(|engine| {
        if let Some(shell_kind) = shell_kind {
            let command =
                call.arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
            engine.evaluate_shell_call(
                shell_kind,
                &permission_names,
                command,
                &call.arguments,
                &context.cwd,
            )
        } else {
            engine.evaluate_call_any(&permission_names, &call.arguments, &context.cwd)
        }
    });

    match engine_decision {
        Some(RuleDecision::Allow) => Ok(PermissionDecision::Allow),
        Some(RuleDecision::Deny) => {
            Ok(PermissionDecision::Deny { reason: "denied by permission rule".into() })
        }
        Some(RuleDecision::Ask) => {
            Ok(PermissionDecision::Ask {
                reason: "approval required by permission rule".into(),
            })
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

/// Extract a human-readable detail from a tool's arguments.
pub(crate) fn tool_detail(name: &str, args: &serde_json::Value) -> String {
    let name_lower = name.to_lowercase();
    match name_lower.as_str() {
        "bash" => {
            args.get("command").and_then(|v| v.as_str()).map(truncate_cmd).unwrap_or_default()
        }
        "read" | "write" | "edit" => args
            .get("file_path")
            .or_else(|| args.get("path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "grep" | "glob" => {
            args.get("pattern").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default()
        }
        "websearch" => {
            args.get("query").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default()
        }
        "webfetch" => args
            .get("url")
            .or_else(|| args.get("urls"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "task" | "agent" => args
            .get("description")
            .or_else(|| args.get("prompt"))
            .and_then(|v| v.as_str())
            .map(truncate_cmd)
            .unwrap_or_default(),
        _ => args
            .get("command")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("path"))
            .or_else(|| args.get("pattern"))
            .or_else(|| args.get("query"))
            .or_else(|| args.get("url"))
            .or_else(|| args.get("description"))
            .and_then(|v| v.as_str())
            .map(truncate_cmd)
            .unwrap_or_default(),
    }
}

pub(crate) fn tool_result_detail(content: &serde_json::Value) -> String {
    if let Some(message) = content.get("message").and_then(|value| value.as_str()) {
        return message.to_string();
    }
    if let Some(error) = content.get("error").and_then(|value| value.as_str()) {
        return error.to_string();
    }
    if let Some(output) = content.get("output").and_then(|value| value.as_str()) {
        return output.to_string();
    }
    content.to_string()
}

fn truncate_cmd(cmd: &str) -> String {
    let first_line = cmd.lines().next().unwrap_or(cmd);
    let mut chars = first_line.chars();
    let truncated: String = chars.by_ref().take(117).collect();
    if chars.next().is_some() {
        format!("{truncated}\u{2026}")
    } else {
        first_line.to_string()
    }
}
