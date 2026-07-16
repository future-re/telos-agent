use crate::error::AgentError;
use crate::model::provider::ModelHint;
use serde::Deserialize;

/// Where an agent definition came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSource {
    BuiltIn,
    Project { path: String },
    Plugin { plugin: String, path: String },
    User { path: String },
}

/// Isolation mode requested by an agent definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentIsolation {
    #[default]
    None,
    Worktree,
}

/// Registry-ready subagent definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub model_hint: Option<ModelHint>,
    pub max_iterations: Option<usize>,
    pub background: bool,
    pub isolation: AgentIsolation,
    pub initial_prompt: Option<String>,
    pub permission_mode: Option<String>,
    pub skills: Vec<String>,
    pub effort: Option<String>,
    pub source: AgentSource,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    disallowed_tools: Vec<String>,
    model: Option<String>,
    max_iterations: Option<usize>,
    #[serde(default)]
    background: bool,
    isolation: Option<String>,
    initial_prompt: Option<String>,
    permission_mode: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
    effort: Option<String>,
}

impl AgentDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        system_prompt: impl Into<String>,
        source: AgentSource,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            system_prompt: system_prompt.into(),
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            model_hint: None,
            max_iterations: None,
            background: false,
            isolation: AgentIsolation::None,
            initial_prompt: None,
            permission_mode: None,
            skills: Vec::new(),
            effort: None,
            source,
        }
    }

    pub fn from_markdown(markdown: &str, source: AgentSource) -> Result<Self, AgentError> {
        let (frontmatter, body) = split_frontmatter(markdown).ok_or_else(|| {
            AgentError::Validation("agent markdown missing YAML frontmatter".into())
        })?;
        let frontmatter: AgentFrontmatter = serde_yaml::from_str(frontmatter)
            .map_err(|err| AgentError::Validation(format!("invalid agent frontmatter: {err}")))?;

        let name = required_frontmatter_string(frontmatter.name, "name")?;
        let description = required_frontmatter_string(frontmatter.description, "description")?;
        let model_hint = match frontmatter.model {
            Some(model) => parse_model_hint(&model)?,
            None => None,
        };
        let isolation = match frontmatter
            .isolation
            .as_deref()
            .unwrap_or("none")
            .to_ascii_lowercase()
            .as_str()
        {
            "none" => AgentIsolation::None,
            "worktree" => AgentIsolation::Worktree,
            other => {
                return Err(AgentError::Validation(format!(
                    "invalid agent isolation `{other}`; expected `none` or `worktree`"
                )));
            }
        };

        Ok(Self {
            name,
            description,
            system_prompt: body.trim().to_string(),
            allowed_tools: frontmatter.tools,
            disallowed_tools: frontmatter.disallowed_tools,
            model_hint,
            max_iterations: frontmatter.max_iterations,
            background: frontmatter.background,
            isolation,
            initial_prompt: optional_trimmed_string(frontmatter.initial_prompt),
            permission_mode: optional_trimmed_string(frontmatter.permission_mode),
            skills: frontmatter.skills,
            effort: optional_trimmed_string(frontmatter.effort),
            source,
        })
    }
}

fn split_frontmatter(markdown: &str) -> Option<(&str, &str)> {
    let rest = markdown.strip_prefix("---\n")?;
    let (frontmatter, body) = rest.split_once("\n---")?;
    Some((frontmatter, body.trim_start_matches(['\r', '\n'])))
}

fn required_frontmatter_string(value: Option<String>, key: &str) -> Result<String, AgentError> {
    let Some(value) = value else {
        return Err(AgentError::Validation(format!("missing required `{key}`")));
    };
    let value = value.trim();
    if value.is_empty() {
        Err(AgentError::Validation(format!("missing required `{key}`")))
    } else {
        Ok(value.to_string())
    }
}

fn optional_trimmed_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        if value.is_empty() { None } else { Some(value.to_string()) }
    })
}

pub(crate) fn parse_model_hint(raw: &str) -> Result<Option<ModelHint>, AgentError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "inherit" | "default" | "none" => Ok(None),
        "thinking" => Ok(Some(ModelHint::Thinking)),
        "execution" => Ok(Some(ModelHint::Execution)),
        "recovery" => Ok(Some(ModelHint::Recovery)),
        "summarization" | "summary" => Ok(Some(ModelHint::Summarization)),
        other => Err(AgentError::Validation(format!(
            "invalid agent model `{other}`; expected thinking, execution, recovery, summarization, or inherit"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::provider::ModelHint;

    #[test]
    fn parses_agent_markdown_frontmatter() {
        let markdown = r#"---
name: Explore
description: Use for broad read-only exploration.
tools: [Read, Grep, Glob]
disallowedTools: [Write, Edit]
model: execution
maxIterations: 8
background: true
isolation: worktree
initialPrompt: Read README.md first.
permissionMode: plan
skills: [debug, verify]
effort: high
---
You inspect the codebase and report findings.
"#;

        let definition = AgentDefinition::from_markdown(
            markdown,
            AgentSource::Project { path: "agents/explore.md".into() },
        )
        .unwrap();

        assert_eq!(definition.name, "Explore");
        assert_eq!(definition.description, "Use for broad read-only exploration.");
        assert_eq!(definition.system_prompt, "You inspect the codebase and report findings.");
        assert_eq!(definition.allowed_tools, vec!["Read", "Grep", "Glob"]);
        assert_eq!(definition.disallowed_tools, vec!["Write", "Edit"]);
        assert_eq!(definition.model_hint, Some(ModelHint::Execution));
        assert_eq!(definition.max_iterations, Some(8));
        assert!(definition.background);
        assert_eq!(definition.isolation, AgentIsolation::Worktree);
        assert_eq!(definition.initial_prompt.as_deref(), Some("Read README.md first."));
        assert_eq!(definition.permission_mode.as_deref(), Some("plan"));
        assert_eq!(definition.skills, vec!["debug", "verify"]);
        assert_eq!(definition.effort.as_deref(), Some("high"));
    }

    #[test]
    fn rejects_missing_required_agent_frontmatter() {
        let missing_name = r#"---
description: Missing name.
---
Prompt.
"#;
        let error = AgentDefinition::from_markdown(
            missing_name,
            AgentSource::Project { path: "agents/bad.md".into() },
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing required `name`"));

        let missing_description = r#"---
name: Bad
---
Prompt.
"#;
        let error = AgentDefinition::from_markdown(
            missing_description,
            AgentSource::Project { path: "agents/bad.md".into() },
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing required `description`"));
    }

    #[test]
    fn parses_model_hint_aliases() {
        assert_eq!(parse_model_hint("thinking").unwrap(), Some(ModelHint::Thinking));
        assert_eq!(parse_model_hint("execution").unwrap(), Some(ModelHint::Execution));
        assert_eq!(parse_model_hint("recovery").unwrap(), Some(ModelHint::Recovery));
        assert_eq!(parse_model_hint("summarization").unwrap(), Some(ModelHint::Summarization));
        assert!(parse_model_hint("inherit").unwrap().is_none());
        assert!(parse_model_hint("unknown").is_err());
    }
}
