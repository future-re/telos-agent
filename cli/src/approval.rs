use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use telos_agent::{ApprovalDecision, ApprovalHandler, ApprovalRequest};

/// Approval policy for tool calls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalPolicy {
    /// Always allow without prompting.
    AlwaysAllow,
    /// Always prompt interactively.
    #[default]
    AlwaysAsk,
    /// Always deny without prompting.
    AlwaysDeny,
}

impl ApprovalPolicy {
    /// Returns `true` if the policy is [`AlwaysAllow`](ApprovalPolicy::AlwaysAllow).
    pub fn is_allow(self) -> bool {
        matches!(self, Self::AlwaysAllow)
    }

    /// Returns a decision based on the policy.
    ///
    /// - `AlwaysAllow` returns `Some(Allow)`.
    /// - `AlwaysDeny` returns `Some(Deny { .. })`.
    /// - `AlwaysAsk` returns `None` (meaning "delegate to the interactive handler").
    pub fn decide(self, _tool_name: &str, _args: Value) -> Option<ApprovalDecision> {
        match self {
            Self::AlwaysAllow => Some(ApprovalDecision::Allow),
            Self::AlwaysDeny => {
                Some(ApprovalDecision::Deny { reason: "policy: always deny".into() })
            }
            Self::AlwaysAsk => None,
        }
    }
}

/// Per-tool policy configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PolicyConfig {
    /// Default policy applied when no tool-specific policy is set.
    pub default: ApprovalPolicy,
    /// Tool-specific overrides keyed by tool name.
    pub policies: HashMap<String, ApprovalPolicy>,
}

impl PolicyConfig {
    fn lookup_policy(&self, tool_name: &str) -> Option<ApprovalPolicy> {
        self.policies
            .get(tool_name)
            .copied()
            .or_else(|| self.policies.get(&tool_name.to_lowercase()).copied())
    }

    /// Returns the effective policy for the given tool name.
    ///
    /// Looks up a tool-specific override first; falls back to `default`.
    pub fn policy_for(&self, tool_name: &str) -> ApprovalPolicy {
        self.lookup_policy(tool_name).unwrap_or(self.default)
    }

    /// Returns the effective policy for any accepted name for a tool.
    ///
    /// This lets config use either canonical names (`Bash`) or aliases
    /// (`shell`), with case-insensitive matching for documented examples.
    pub fn policy_for_any<'a>(
        &self,
        tool_names: impl IntoIterator<Item = &'a str>,
    ) -> ApprovalPolicy {
        for name in tool_names {
            if let Some(policy) = self.lookup_policy(name) {
                return policy;
            }
        }
        self.default
    }
}

/// Interactive terminal approval handler.
///
/// Presents the tool call to the user and reads a decision from stdin:
/// - `y` / `yes` / empty → Allow
/// - `n` / `no` → Deny
/// - `m` / `modify` → Prompt for modified arguments (JSON)
///
/// When a [`PolicyConfig`] is set, the handler checks the policy first and
/// may short-circuit (Allow / Deny) without prompting.
#[derive(Debug, Clone, Default)]
pub struct TerminalApprovalHandler {
    /// Optional policy configuration for fast-path decisions.
    pub policy: Option<PolicyConfig>,
}

impl TerminalApprovalHandler {
    /// Create a new handler with an optional policy configuration.
    pub fn new(policy: Option<PolicyConfig>) -> Self {
        Self { policy }
    }

    /// Set or replace the policy configuration at runtime.
    pub fn set_policy(&mut self, policy: Option<PolicyConfig>) {
        self.policy = policy;
    }
}

#[async_trait]
impl ApprovalHandler for TerminalApprovalHandler {
    async fn ask(&self, request: ApprovalRequest) -> ApprovalDecision {
        // Check policy first for a fast-path decision.
        if let Some(ref config) = self.policy {
            let names = std::iter::once(request.tool_name.as_str())
                .chain(request.invocation_names.iter().map(String::as_str));
            let tool_policy = config.policy_for_any(names);
            if let Some(decision) =
                tool_policy.decide(&request.tool_name, request.arguments.clone())
            {
                return decision;
            }
            // AlwaysAsk or no applicable policy -> fall through to interactive.
        }

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
