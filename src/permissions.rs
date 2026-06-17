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
        self.evaluate_call(tool_name, &serde_json::Value::Null, std::path::Path::new("."))
    }

    /// Evaluate a tool call against all rules. Later rules override earlier ones.
    pub fn evaluate_call(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        cwd: &std::path::Path,
    ) -> Option<RuleDecision> {
        self.evaluate_call_any(&[tool_name], arguments, cwd)
    }

    /// Evaluate a tool call against any accepted tool name for that tool.
    ///
    /// This preserves the usual "last matching rule wins" behavior across
    /// canonical names and legacy aliases.
    pub fn evaluate_call_any(
        &self,
        tool_names: &[&str],
        arguments: &serde_json::Value,
        cwd: &std::path::Path,
    ) -> Option<RuleDecision> {
        let mut result = None;
        for rule in &self.rules {
            if tool_names.iter().any(|tool_name| Self::match_name(&rule.tool_name, tool_name))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- match_name ---

    #[test]
    fn wildcard_star_matches_any_name() {
        assert!(PermissionEngine::match_name("*", "anything"));
        assert!(PermissionEngine::match_name("*", ""));
    }

    #[test]
    fn prefix_wildcard_matches() {
        assert!(PermissionEngine::match_name("Bash*", "Bash"));
        assert!(PermissionEngine::match_name("Bash*", "BashExtended"));
        assert!(!PermissionEngine::match_name("Bash*", "bash_lowercase"));
        assert!(!PermissionEngine::match_name("Bash*", "XxxBash"));
    }

    #[test]
    fn exact_match_no_wildcard() {
        assert!(PermissionEngine::match_name("Write", "Write"));
        assert!(!PermissionEngine::match_name("Write", "write"));
        assert!(!PermissionEngine::match_name("Write", "WriteTool"));
    }

    // --- evaluate (name-only) ---

    #[test]
    fn evaluate_returns_none_when_no_rules() {
        let engine = PermissionEngine::new();
        assert_eq!(engine.evaluate("Read"), None);
    }

    #[test]
    fn evaluate_returns_last_matching_rule() {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::deny_tool("*"));
        engine.add_rule(PermissionRule::allow_tool("Read"));
        assert_eq!(engine.evaluate("Read"), Some(RuleDecision::Allow));
        assert_eq!(engine.evaluate("Write"), Some(RuleDecision::Deny));
    }

    #[test]
    fn evaluate_falls_through_when_no_match() {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::allow_tool("Read"));
        assert_eq!(engine.evaluate("Write"), None);
    }

    // --- evaluate_call_any (aliases) ---

    #[test]
    fn evaluate_call_any_respects_aliases() {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::deny_tool("shell"));
        // The tool is registered as "Bash" with alias "shell" — both should hit.
        assert!(
            engine
                .evaluate_call_any(&["Bash", "shell"], &json!({}), &std::path::Path::new("."))
                .is_some()
        );
    }

    #[test]
    fn evaluate_call_any_last_alias_wins() {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::allow_tool("Bash"));
        engine.add_rule(PermissionRule::deny_tool("shell"));
        // "Bash" and "shell" are both accepted names for the same call;
        // the last matching rule wins.
        assert_eq!(
            engine.evaluate_call_any(&["Bash", "shell"], &json!({}), &std::path::Path::new(".")),
            Some(RuleDecision::Deny)
        );
    }

    // --- command_prefix ---

    #[test]
    fn command_prefix_matches_when_set() {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::allow_tool("Bash").command_prefix("git "));
        assert_eq!(
            engine.evaluate_call(
                "Bash",
                &json!({"command": "git status"}),
                &std::path::Path::new(".")
            ),
            Some(RuleDecision::Allow)
        );
    }

    #[test]
    fn command_prefix_no_match_when_wrong_command() {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::allow_tool("Bash").command_prefix("ls "));
        assert_eq!(
            engine.evaluate_call(
                "Bash",
                &json!({"command": "rm -rf /"}),
                &std::path::Path::new(".")
            ),
            None
        );
    }

    #[test]
    fn command_prefix_does_not_match_without_command_key() {
        let mut engine = PermissionEngine::new();
        engine.add_rule(PermissionRule::allow_tool("Bash").command_prefix("ls "));
        assert_eq!(engine.evaluate_call("Bash", &json!({}), &std::path::Path::new(".")), None);
    }

    // --- cwd_prefix ---

    #[test]
    fn cwd_prefix_matches_when_cwd_under_prefix() {
        let mut engine = PermissionEngine::new();
        engine.add_rule(
            PermissionRule::allow_tool("Write").cwd_prefix(std::path::PathBuf::from("/safe")),
        );
        assert_eq!(
            engine.evaluate_call("Write", &json!({}), &std::path::Path::new("/safe/sub/dir")),
            Some(RuleDecision::Allow)
        );
    }

    #[test]
    fn cwd_prefix_rejects_cwd_outside_prefix() {
        let mut engine = PermissionEngine::new();
        engine.add_rule(
            PermissionRule::allow_tool("Write").cwd_prefix(std::path::PathBuf::from("/safe")),
        );
        assert_eq!(
            engine.evaluate_call("Write", &json!({}), &std::path::Path::new("/unsafe")),
            None
        );
    }

    // --- combined filters ---

    #[test]
    fn command_and_cwd_both_applied() {
        let mut engine = PermissionEngine::new();
        engine.add_rule(
            PermissionRule::deny_tool("Bash")
                .command_prefix("rm ")
                .cwd_prefix(std::path::PathBuf::from("/protected")),
        );
        // correct command, wrong dir — no match
        assert_eq!(
            engine.evaluate_call(
                "Bash",
                &json!({"command": "rm file"}),
                &std::path::Path::new("/tmp")
            ),
            None
        );
        // wrong command, correct dir — no match
        assert_eq!(
            engine.evaluate_call(
                "Bash",
                &json!({"command": "ls"}),
                &std::path::Path::new("/protected/dir")
            ),
            None
        );
        // both match — rule fires
        assert_eq!(
            engine.evaluate_call(
                "Bash",
                &json!({"command": "rm file"}),
                &std::path::Path::new("/protected/dir")
            ),
            Some(RuleDecision::Deny)
        );
    }
}
