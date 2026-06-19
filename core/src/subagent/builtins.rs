use crate::subagent::definition::{AgentDefinition, AgentSource};

pub fn builtin_agents() -> Vec<AgentDefinition> {
    let mut general = AgentDefinition::new(
        "general-purpose",
        "Use this agent for general multi-step work when no specialized agent fits.",
        "You are a general-purpose subagent. Complete the delegated task fully and report concise results.",
        AgentSource::BuiltIn,
    );
    general.allowed_tools = vec!["*".into()];

    let mut explore = AgentDefinition::new(
        "Explore",
        "Use this agent for broad read-only codebase exploration and research.",
        "You are an explore agent. Search and analyze existing code. Do not edit files. Report findings with file paths and concise evidence.",
        AgentSource::BuiltIn,
    );
    explore.allowed_tools = vec![
        "Read".into(),
        "Grep".into(),
        "Glob".into(),
        "WebFetch".into(),
        "WebSearch".into(),
        "Bash".into(),
    ];
    explore.disallowed_tools = vec!["Write".into(), "Edit".into(), "subagent".into()];
    explore.max_iterations = Some(8);

    let mut plan = AgentDefinition::new(
        "Plan",
        "Use this agent to explore requirements and produce an implementation plan without editing files.",
        "You are a planning agent. Explore the repository, identify constraints, and produce an actionable plan. Do not modify files.",
        AgentSource::BuiltIn,
    );
    plan.allowed_tools = explore.allowed_tools.clone();
    plan.disallowed_tools = explore.disallowed_tools.clone();
    plan.max_iterations = Some(8);

    let mut verification = AgentDefinition::new(
        "Verification",
        "Use this agent to run checks, inspect failures, and verify completed work.",
        "You are a verification agent. Run relevant checks, inspect failures carefully, and report exact verification evidence.",
        AgentSource::BuiltIn,
    );
    verification.allowed_tools = vec![
        "Read".into(),
        "Grep".into(),
        "Glob".into(),
        "Bash".into(),
        "WebFetch".into(),
        "WebSearch".into(),
    ];
    verification.max_iterations = Some(10);

    vec![general, explore, plan, verification]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_agents_have_descriptions_and_prompts() {
        let agents = builtin_agents();
        assert!(agents.iter().all(|agent| !agent.description.is_empty()));
        assert!(agents.iter().all(|agent| !agent.system_prompt.is_empty()));
    }
}
