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
}

impl PermissionRule {
    pub fn allow_tool(name: impl Into<String>) -> Self {
        Self {
            tool_name: name.into(),
            decision: RuleDecision::Allow,
        }
    }

    pub fn deny_tool(name: impl Into<String>) -> Self {
        Self {
            tool_name: name.into(),
            decision: RuleDecision::Deny,
        }
    }

    pub fn ask_tool(name: impl Into<String>) -> Self {
        Self {
            tool_name: name.into(),
            decision: RuleDecision::Ask,
        }
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
        // Later rules override earlier ones (last-match wins)
        let mut result = None;
        for rule in &self.rules {
            if Self::match_name(&rule.tool_name, tool_name) {
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
}
