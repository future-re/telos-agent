#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleDecision {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone)]
pub struct PermissionRule {
    pub tool_name: String,
    pub decision: RuleDecision,
    pub command_prefix: Option<String>,
    pub cwd_prefix: Option<std::path::PathBuf>,
}

impl PermissionRule {
    pub fn allow_tool(name: impl Into<String>) -> Self {
        Self {
            tool_name: name.into(),
            decision: RuleDecision::Allow,
            command_prefix: None,
            cwd_prefix: None,
        }
    }

    pub fn deny_tool(name: impl Into<String>) -> Self {
        Self {
            tool_name: name.into(),
            decision: RuleDecision::Deny,
            command_prefix: None,
            cwd_prefix: None,
        }
    }

    pub fn ask_tool(name: impl Into<String>) -> Self {
        Self {
            tool_name: name.into(),
            decision: RuleDecision::Ask,
            command_prefix: None,
            cwd_prefix: None,
        }
    }

    pub fn command_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.command_prefix = Some(prefix.into());
        self
    }

    pub fn cwd_prefix(mut self, prefix: impl Into<std::path::PathBuf>) -> Self {
        self.cwd_prefix = Some(prefix.into());
        self
    }
}

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

    pub fn add_rule(&mut self, rule: PermissionRule) {
        self.rules.push(rule);
    }

    pub fn evaluate(&self, tool_name: &str) -> Option<RuleDecision> {
        self.evaluate_call(
            tool_name,
            &serde_json::Value::Null,
            std::path::Path::new("."),
        )
    }

    pub fn evaluate_call(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        cwd: &std::path::Path,
    ) -> Option<RuleDecision> {
        // Later rules override earlier ones (last-match wins)
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

    fn match_cwd_prefix(rule: &PermissionRule, cwd: &std::path::Path) -> bool {
        let Some(prefix) = &rule.cwd_prefix else {
            return true;
        };
        cwd.starts_with(prefix)
    }
}
