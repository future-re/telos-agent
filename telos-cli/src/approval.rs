use async_trait::async_trait;
use serde_json::Value;
use telos_agent::{ApprovalDecision, ApprovalHandler, ApprovalRequest};

/// Interactive terminal approval handler.
///
/// Presents the tool call to the user and reads a decision from stdin:
/// - `y` / `yes` / empty → Allow
/// - `n` / `no` → Deny
/// - `m` / `modify` → Prompt for modified arguments (JSON)
#[derive(Debug, Clone, Default)]
pub struct TerminalApprovalHandler;

#[async_trait]
impl ApprovalHandler for TerminalApprovalHandler {
    async fn ask(&self, request: ApprovalRequest) -> ApprovalDecision {
        eprintln!();
        eprintln!("Approval required: {}", request.tool_name);
        if !request.reason.is_empty() {
            eprintln!("Reason: {}", request.reason);
        }
        eprintln!(
            "Arguments: {}",
            serde_json::to_string_pretty(&request.arguments).unwrap_or_default()
        );
        eprintln!("Allow? [y/n/m] (default: n)");

        match read_line().await {
            Some(line) => parse_decision(&line, &request.arguments),
            None => ApprovalDecision::Deny { reason: "no input received".into() },
        }
    }
}

async fn read_line() -> Option<String> {
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let mut line = String::new();
    match tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line).await {
        Ok(0) => None,
        Ok(_) => Some(line.trim().to_lowercase()),
        Err(_) => None,
    }
}

fn parse_decision(input: &str, original_args: &Value) -> ApprovalDecision {
    match input {
        "y" | "yes" | "" => ApprovalDecision::Allow,
        "m" | "modify" => prompt_modified_arguments(original_args),
        _ => ApprovalDecision::Deny { reason: "user denied".into() },
    }
}

fn prompt_modified_arguments(_original_args: &Value) -> ApprovalDecision {
    eprintln!("Enter modified arguments as JSON (empty to deny):");
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(_) if line.trim().is_empty() => {
            ApprovalDecision::Deny { reason: "modification cancelled".into() }
        }
        Ok(_) => match serde_json::from_str(line.trim()) {
            Ok(args) => ApprovalDecision::Modify { arguments: args },
            Err(e) => {
                eprintln!("Invalid JSON: {e}");
                ApprovalDecision::Deny { reason: "invalid modified arguments".into() }
            }
        },
        Err(_) => ApprovalDecision::Deny { reason: "failed to read modification".into() },
    }
}
