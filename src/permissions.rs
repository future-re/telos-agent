//! Rule-based permission engine.
//!
//! [`PermissionEngine`] evaluates an ordered list of [`PermissionRule`]s
//! against a tool call. Rules are matched on tool name (with optional `*`
//! wildcard suffix), a command prefix (for shell-style tools), and a cwd
//! prefix. The result of the **last matching rule wins** — order rules from
//! general to specific.

/// Outcome of a permission check for a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleDecision {
    /// Tool may run without further approval.
    Allow,
    /// Tool is forbidden; result is delivered to the model as an error.
    Deny,
    /// Defer to the host (typically a human approval prompt).
    Ask,
}

/// A single permission rule. Use the constructor / builder methods to build one.
#[derive(Debug, Clone)]
pub struct PermissionRule {
    /// Tool name to match. Trailing `*` makes it a prefix pattern; `*` alone matches any tool.
    pub tool_name: String,
    /// Decision applied when this rule matches.
    pub decision: RuleDecision,
    /// If set, the rule only matches when the call's `command` argument starts with this prefix.
    pub command_prefix: Option<String>,
    /// If set, the rule only matches when the runtime cwd is inside this directory.
    pub cwd_prefix: Option<std::path::PathBuf>,
}

impl PermissionRule {
    /// Build an `Allow` rule for the given tool name.
    pub fn allow_tool(name: impl Into<String>) -> Self {
        Self {
            tool_name: name.into(),
            decision: RuleDecision::Allow,
            command_prefix: None,
            cwd_prefix: None,
        }
    }

    /// Build a `Deny` rule for the given tool name.
    pub fn deny_tool(name: impl Into<String>) -> Self {
        Self {
            tool_name: name.into(),
            decision: RuleDecision::Deny,
            command_prefix: None,
            cwd_prefix: None,
        }
    }

    /// Build an `Ask` rule for the given tool name.
    pub fn ask_tool(name: impl Into<String>) -> Self {
        Self {
            tool_name: name.into(),
            decision: RuleDecision::Ask,
            command_prefix: None,
            cwd_prefix: None,
        }
    }

    /// Narrow the rule to calls whose `command` argument starts with `prefix`.
    pub fn command_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.command_prefix = Some(prefix.into());
        self
    }

    /// Narrow the rule to calls executed inside (or below) `prefix`.
    pub fn cwd_prefix(mut self, prefix: impl Into<std::path::PathBuf>) -> Self {
        self.cwd_prefix = Some(prefix.into());
        self
    }
}

/// Ordered list of permission rules consulted by the executor before every tool call.
#[derive(Debug, Clone)]
pub struct PermissionEngine {
    rules: Vec<PermissionRule>,
}

impl Default for PermissionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PermissionEngine {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Append a rule; later rules override earlier ones when both match.
    pub fn add_rule(&mut self, rule: PermissionRule) {
        self.rules.push(rule);
    }

    /// Evaluate a tool by name only — ignores command/cwd filters.
    ///
    /// Returns `None` if no rule matches, leaving the decision to the tool itself.
    pub fn evaluate(&self, tool_name: &str) -> Option<RuleDecision> {
        self.evaluate_call(
            tool_name,
            &serde_json::Value::Null,
            std::path::Path::new("."),
        )
    }

    /// Evaluate a tool call against all rules. Later rules override earlier ones.
    pub fn evaluate_call(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        cwd: &std::path::Path,
    ) -> Option<RuleDecision> {
        let mut result = None;
        for rule in &self.rules {
            if Self::match_name(&rule.tool_name, tool_name)
                && Self::match_command_prefix(rule, arguments)
                && Self::match_cwd_prefix(rule, cwd)
            {
                result = Some(rule.decision.clone());
            }
        }
        result
    }

    /// Match `pattern` against `name` with `*` wildcard support (trailing or solo).
    fn match_name(pattern: &str, name: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        if pattern.ends_with('*') {
            let prefix = &pattern[..pattern.len() - 1];
            return name.starts_with(prefix);
        }
        pattern == name
    }

    /// True if no prefix is configured, or if `arguments.command` (trimmed) starts with it.
    fn match_command_prefix(rule: &PermissionRule, arguments: &serde_json::Value) -> bool {
        let Some(prefix) = &rule.command_prefix else {
            return true;
        };
        arguments
            .get("command")
            .and_then(|value| value.as_str())
            .map(|command| command.trim_start().starts_with(prefix))
            .unwrap_or(false)
    }

    /// True if no prefix is configured, or if `cwd` lies under it.
    fn match_cwd_prefix(rule: &PermissionRule, cwd: &std::path::Path) -> bool {
        let Some(prefix) = &rule.cwd_prefix else {
            return true;
        };
        cwd.starts_with(prefix)
    }
}
