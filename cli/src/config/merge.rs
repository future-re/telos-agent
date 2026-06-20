use super::{
    AgentSection, ApprovalSection, DiagnosticsGithubSection, DiagnosticsSection, FileConfig,
    TuiSection,
};

/// Merge two config layers. `project` values override `user` values.
/// Fields set to `None` on the project layer fall through to the user layer.
pub fn merge_configs(user: Option<FileConfig>, project: Option<FileConfig>) -> FileConfig {
    let agent = merge_agent(
        user.as_ref().and_then(|c| c.agent.as_ref()),
        project.as_ref().and_then(|c| c.agent.as_ref()),
    );
    let approval = merge_approval(
        user.as_ref().and_then(|c| c.approval.as_ref()),
        project.as_ref().and_then(|c| c.approval.as_ref()),
    );
    let auto_mode = project
        .as_ref()
        .and_then(|c| c.auto_mode)
        .or_else(|| user.as_ref().and_then(|c| c.auto_mode));
    let diagnostics = merge_diagnostics(
        user.as_ref().and_then(|c| c.diagnostics.as_ref()),
        project.as_ref().and_then(|c| c.diagnostics.as_ref()),
    );
    let tui = merge_tui(
        user.as_ref().and_then(|c| c.tui.as_ref()),
        project.as_ref().and_then(|c| c.tui.as_ref()),
    );
    let env = match (user.and_then(|c| c.env), project.and_then(|c| c.env)) {
        (Some(mut u), Some(p)) => {
            u.extend(p);
            Some(u)
        }
        (Some(u), None) => Some(u),
        (None, Some(p)) => Some(p),
        (None, None) => None,
    };

    FileConfig { agent, approval, diagnostics, tui, env, auto_mode }
}

fn merge_agent(
    user: Option<&AgentSection>,
    project: Option<&AgentSection>,
) -> Option<AgentSection> {
    match (user, project) {
        (None, None) => None,
        (Some(u), None) => Some(AgentSection {
            model: u.model.clone(),
            provider: u.provider.clone(),
            max_iterations: u.max_iterations,
            models: u.models.clone(),
            default_shell: u.default_shell,
        }),
        (None, Some(p)) => Some(AgentSection {
            model: p.model.clone(),
            provider: p.provider.clone(),
            max_iterations: p.max_iterations,
            models: p.models.clone(),
            default_shell: p.default_shell,
        }),
        (Some(u), Some(p)) => Some(AgentSection {
            model: p.model.clone().or_else(|| u.model.clone()),
            provider: p.provider.clone().or_else(|| u.provider.clone()),
            max_iterations: p.max_iterations.or(u.max_iterations),
            models: p.models.clone().or_else(|| u.models.clone()),
            default_shell: p.default_shell.or(u.default_shell),
        }),
    }
}

fn merge_approval(
    user: Option<&ApprovalSection>,
    project: Option<&ApprovalSection>,
) -> Option<ApprovalSection> {
    match (user, project) {
        (None, None) => None,
        (Some(u), None) => Some(ApprovalSection {
            default_policy: u.default_policy.clone(),
            policies: u.policies.clone(),
        }),
        (None, Some(p)) => Some(ApprovalSection {
            default_policy: p.default_policy.clone(),
            policies: p.policies.clone(),
        }),
        (Some(u), Some(p)) => Some(ApprovalSection {
            default_policy: p.default_policy.clone().or_else(|| u.default_policy.clone()),
            policies: p.policies.clone().or_else(|| u.policies.clone()),
        }),
    }
}

fn merge_diagnostics(
    user: Option<&DiagnosticsSection>,
    project: Option<&DiagnosticsSection>,
) -> Option<DiagnosticsSection> {
    match (user, project) {
        (None, None) => None,
        (Some(u), None) => Some(u.clone()),
        (None, Some(p)) => Some(p.clone()),
        (Some(u), Some(p)) => Some(DiagnosticsSection {
            enabled: p.enabled.or(u.enabled),
            retention_days: p.retention_days.or(u.retention_days),
            github: merge_diagnostics_github(u.github.as_ref(), p.github.as_ref()),
        }),
    }
}

fn merge_diagnostics_github(
    user: Option<&DiagnosticsGithubSection>,
    project: Option<&DiagnosticsGithubSection>,
) -> Option<DiagnosticsGithubSection> {
    match (user, project) {
        (None, None) => None,
        (Some(u), None) => Some(u.clone()),
        (None, Some(p)) => Some(p.clone()),
        (Some(u), Some(p)) => Some(DiagnosticsGithubSection {
            enabled: p.enabled.or(u.enabled),
            repository: p.repository.clone().or_else(|| u.repository.clone()),
            interval_hours: p.interval_hours.or(u.interval_hours),
            min_occurrences: p.min_occurrences.or(u.min_occurrences),
        }),
    }
}

fn merge_tui(user: Option<&TuiSection>, project: Option<&TuiSection>) -> Option<TuiSection> {
    match (user, project) {
        (None, None) => None,
        (Some(u), None) => Some(u.clone()),
        (None, Some(p)) => Some(p.clone()),
        (Some(u), Some(p)) => Some(TuiSection { density: p.density.or(u.density) }),
    }
}
