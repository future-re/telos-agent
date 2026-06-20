use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use telos_agent::{AgentConfig, JsonlToolDiagnosticsSink, ToolFailureEvent, ToolFailureKind};

use crate::config::FileConfig;

#[derive(Debug, Clone)]
pub struct DiagnosticsRuntime {
    pub dir: PathBuf,
    pub events_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsProcessReport {
    pub groups: usize,
    pub drafts: Vec<PathBuf>,
    pub github_issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureGroup {
    pub signature: String,
    pub tool_name: String,
    pub failure_kind: ToolFailureKind,
    pub occurrences: usize,
    pub first_seen_unix_secs: u64,
    pub last_seen_unix_secs: u64,
    pub argument_summary: String,
    pub error_summary: String,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct GithubIssueReporter {
    base_url: String,
    repository: String,
    token: String,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct GithubIssueRequest {
    title: String,
    body: String,
    labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GithubIssueResponse {
    html_url: Option<String>,
}

impl GithubIssueReporter {
    pub fn new(
        base_url: impl Into<String>,
        repository: impl Into<String>,
        token: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            repository: repository.into(),
            token: token.into(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn create_issue(&self, group: &FailureGroup) -> Result<Option<String>> {
        let url = format!("{}/repos/{}/issues", self.base_url, self.repository);
        let request = GithubIssueRequest {
            title: issue_title(group),
            body: issue_body(group),
            labels: vec!["tool-failure".into(), "automated".into(), "privacy-sanitized".into()],
        };
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.token)
            .header(reqwest::header::USER_AGENT, "telos-cli")
            .json(&request)
            .send()
            .await
            .context("failed to create GitHub issue")?
            .error_for_status()
            .context("GitHub issue creation failed")?;
        let body: GithubIssueResponse =
            response.json().await.context("failed to parse GitHub issue response")?;
        Ok(body.html_url)
    }
}

pub fn diagnostics_dir(project_root: Option<&Path>) -> Result<PathBuf> {
    match project_root {
        Some(root) => Ok(root.join(".telos").join("diagnostics")),
        None => {
            let base = dirs::data_dir().context("could not determine data directory")?;
            Ok(base.join("telos").join("diagnostics"))
        }
    }
}

pub fn configure_tool_diagnostics(
    agent_config: &mut AgentConfig,
    file_config: &FileConfig,
    project_root: Option<&Path>,
) -> Result<Option<DiagnosticsRuntime>> {
    let enabled =
        file_config.diagnostics.as_ref().and_then(|section| section.enabled).unwrap_or(true);
    if !enabled {
        agent_config.tool_diagnostics = None;
        return Ok(None);
    }

    let dir = diagnostics_dir(project_root)?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create diagnostics directory: {}", dir.display()))?;
    let events_path = dir.join("tool-failures.jsonl");
    agent_config.tool_diagnostics =
        Some(Arc::new(JsonlToolDiagnosticsSink::new(events_path.clone())));
    Ok(Some(DiagnosticsRuntime { dir, events_path }))
}

pub fn aggregate_failures(events_path: &Path) -> Result<Vec<FailureGroup>> {
    if !events_path.exists() {
        return Ok(Vec::new());
    }
    let contents = std::fs::read_to_string(events_path)
        .with_context(|| format!("failed to read diagnostics events: {}", events_path.display()))?;
    let mut groups = std::collections::BTreeMap::<String, FailureGroup>::new();
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let event: ToolFailureEvent = serde_json::from_str(line).with_context(|| {
            format!("failed to parse diagnostics event in {}", events_path.display())
        })?;
        groups
            .entry(event.signature.clone())
            .and_modify(|group| {
                group.occurrences += 1;
                group.first_seen_unix_secs =
                    group.first_seen_unix_secs.min(event.timestamp_unix_secs);
                group.last_seen_unix_secs =
                    group.last_seen_unix_secs.max(event.timestamp_unix_secs);
            })
            .or_insert_with(|| FailureGroup {
                signature: event.signature,
                tool_name: event.tool_name,
                failure_kind: event.failure_kind,
                occurrences: 1,
                first_seen_unix_secs: event.timestamp_unix_secs,
                last_seen_unix_secs: event.timestamp_unix_secs,
                argument_summary: event.argument_summary,
                error_summary: event.error_summary,
                exit_code: event.exit_code,
            });
    }
    Ok(groups.into_values().collect())
}

pub fn write_issue_drafts(
    diagnostics_dir: &Path,
    groups: &[FailureGroup],
    min_occurrences: usize,
) -> Result<Vec<PathBuf>> {
    let issues_dir = diagnostics_dir.join("issues");
    std::fs::create_dir_all(&issues_dir).with_context(|| {
        format!("failed to create issue draft directory: {}", issues_dir.display())
    })?;
    let mut paths = Vec::new();
    for group in groups.iter().filter(|group| group.occurrences >= min_occurrences) {
        let path = issues_dir.join(format!("{}.md", signature_slug(&group.signature)));
        std::fs::write(&path, issue_body(group))
            .with_context(|| format!("failed to write issue draft: {}", path.display()))?;
        paths.push(path);
    }
    Ok(paths)
}

pub async fn process_diagnostics(
    runtime: &DiagnosticsRuntime,
    file_config: &FileConfig,
) -> Result<DiagnosticsProcessReport> {
    let groups = aggregate_failures(&runtime.events_path)?;
    let min_occurrences = diagnostics_min_occurrences(file_config);
    let reportable = groups
        .iter()
        .filter(|group| group.occurrences >= min_occurrences)
        .cloned()
        .collect::<Vec<_>>();
    let github = file_config.diagnostics.as_ref().and_then(|section| section.github.as_ref());

    if !github.and_then(|github| github.enabled).unwrap_or(false) {
        let drafts = write_issue_drafts(&runtime.dir, &reportable, min_occurrences)?;
        return Ok(DiagnosticsProcessReport {
            groups: groups.len(),
            drafts,
            github_issues: Vec::new(),
        });
    }

    let Some(token) = github_token(file_config) else {
        let drafts = write_issue_drafts(&runtime.dir, &reportable, min_occurrences)?;
        tracing::warn!("diagnostics GitHub reporting enabled but GITHUB_TOKEN is not configured");
        return Ok(DiagnosticsProcessReport {
            groups: groups.len(),
            drafts,
            github_issues: Vec::new(),
        });
    };

    let github = github.expect("checked enabled GitHub config");
    let repository = github.repository.as_deref().unwrap_or("future-re/telos-agent").to_string();
    let interval_secs = github.interval_hours.unwrap_or(24).saturating_mul(60 * 60);
    let state_path = runtime.dir.join("reported-state.json");
    let mut state = ReportState::load(&state_path)?;
    let now = unix_timestamp();
    let reporter = GithubIssueReporter::new("https://api.github.com", repository, token);
    let mut github_issues = Vec::new();

    for group in reportable {
        if !state.should_report(&group.signature, now, interval_secs) {
            continue;
        }
        if let Some(url) = reporter.create_issue(&group).await? {
            github_issues.push(url);
        }
        state.mark_reported(&group.signature, now);
    }
    state.save(&state_path)?;

    Ok(DiagnosticsProcessReport { groups: groups.len(), drafts: Vec::new(), github_issues })
}

fn issue_body(group: &FailureGroup) -> String {
    format!(
        "# Tool failure: {} {}\n\n\
This is a locally generated, privacy-sanitized tool failure summary.\n\n\
- Signature: `{}`\n\
- Tool: `{}`\n\
- Failure kind: `{}`\n\
- Occurrences: {}\n\
- First seen: {}\n\
- Last seen: {}\n\
- Exit code: {}\n\
- Argument summary: `{}`\n\n\
## Sanitized Error Summary\n\n\
```\n{}\n```\n",
        group.tool_name,
        group.failure_kind.as_str(),
        group.signature,
        group.tool_name,
        group.failure_kind.as_str(),
        group.occurrences,
        group.first_seen_unix_secs,
        group.last_seen_unix_secs,
        group.exit_code.map(|code| code.to_string()).unwrap_or_else(|| "n/a".into()),
        group.argument_summary,
        group.error_summary
    )
}

fn issue_title(group: &FailureGroup) -> String {
    format!(
        "Tool failure: {} {} ({})",
        group.tool_name,
        group.failure_kind.as_str(),
        signature_slug(&group.signature)
    )
}

fn diagnostics_min_occurrences(file_config: &FileConfig) -> usize {
    file_config
        .diagnostics
        .as_ref()
        .and_then(|section| section.github.as_ref())
        .and_then(|github| github.min_occurrences)
        .unwrap_or(3)
}

fn github_token(file_config: &FileConfig) -> Option<String> {
    std::env::var("GITHUB_TOKEN").ok().filter(|token| !token.trim().is_empty()).or_else(|| {
        file_config
            .env
            .as_ref()
            .and_then(|env| env.get("GITHUB_TOKEN"))
            .filter(|token| !token.trim().is_empty())
            .cloned()
    })
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ReportState {
    signatures: std::collections::BTreeMap<String, u64>,
}

impl ReportState {
    fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path).with_context(|| {
            format!("failed to read diagnostics report state: {}", path.display())
        })?;
        serde_json::from_str(&contents).with_context(|| {
            format!("failed to parse diagnostics report state: {}", path.display())
        })
    }

    fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create diagnostics state directory: {}", parent.display())
            })?;
        }
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path, contents).with_context(|| {
            format!("failed to write diagnostics report state: {}", path.display())
        })
    }

    fn should_report(&self, signature: &str, now: u64, interval_secs: u64) -> bool {
        self.signatures
            .get(signature)
            .map(|last| now.saturating_sub(*last) >= interval_secs)
            .unwrap_or(true)
    }

    fn mark_reported(&mut self, signature: &str, timestamp: u64) {
        self.signatures.insert(signature.to_string(), timestamp);
    }
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn signature_slug(signature: &str) -> String {
    let mut slug = signature
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '-' })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    slug.trim_matches('-').chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FileConfig;
    use telos_agent::AgentConfig;

    #[test]
    fn diagnostics_dir_uses_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let path = diagnostics_dir(Some(dir.path())).unwrap();
        assert_eq!(path, dir.path().join(".telos").join("diagnostics"));
    }

    #[test]
    fn default_cli_diagnostics_installs_local_sink() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = AgentConfig::default();
        let runtime =
            configure_tool_diagnostics(&mut agent, &FileConfig::default(), Some(dir.path()))
                .unwrap();
        assert!(runtime.is_some());
        assert!(agent.tool_diagnostics.is_some());
    }

    #[test]
    fn aggregates_repeated_failures() {
        let dir = tempfile::tempdir().unwrap();
        let events_path = dir.path().join("tool-failures.jsonl");
        let first = sample_event(10, "sig-1");
        let second = sample_event(20, "sig-1");
        std::fs::write(
            &events_path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&first).unwrap(),
                serde_json::to_string(&second).unwrap()
            ),
        )
        .unwrap();

        let groups = aggregate_failures(&events_path).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].signature, "sig-1");
        assert_eq!(groups[0].occurrences, 2);
        assert_eq!(groups[0].first_seen_unix_secs, 10);
        assert_eq!(groups[0].last_seen_unix_secs, 20);
    }

    #[test]
    fn creates_issue_draft_when_github_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let group = FailureGroup {
            signature: "Bash:execution_error:exit=101:cargo test:failed".into(),
            tool_name: "Bash".into(),
            failure_kind: telos_agent::ToolFailureKind::ExecutionError,
            occurrences: 3,
            first_seen_unix_secs: 10,
            last_seen_unix_secs: 30,
            argument_summary: "cargo test".into(),
            error_summary: "failed".into(),
            exit_code: Some(101),
        };

        let drafts = write_issue_drafts(dir.path(), &[group], 2).unwrap();
        assert_eq!(drafts.len(), 1);
        let body = std::fs::read_to_string(&drafts[0]).unwrap();
        assert!(body.contains("Tool failure: Bash execution_error"));
        assert!(body.contains("Occurrences: 3"));
        assert!(!body.contains("/home/"));
    }

    #[tokio::test]
    async fn github_reporter_posts_sanitized_issue() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/future-re/telos-agent/issues"))
            .and(header("authorization", "Bearer ghp_test"))
            .and(body_json(serde_json::json!({
                "title": "Tool failure: Bash execution_error (bash-execution-error)",
                "body": "# Tool failure: Bash execution_error\n\nThis is a locally generated, privacy-sanitized tool failure summary.\n\n- Signature: `Bash:execution_error`\n- Tool: `Bash`\n- Failure kind: `execution_error`\n- Occurrences: 3\n- First seen: 10\n- Last seen: 30\n- Exit code: 101\n- Argument summary: `cargo test`\n\n## Sanitized Error Summary\n\n```\nfailed\n```\n",
                "labels": ["tool-failure", "automated", "privacy-sanitized"]
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "html_url": "https://github.com/future-re/telos-agent/issues/1"
            })))
            .mount(&server)
            .await;

        let reporter = GithubIssueReporter::new(server.uri(), "future-re/telos-agent", "ghp_test");
        let group = FailureGroup {
            signature: "Bash:execution_error".into(),
            tool_name: "Bash".into(),
            failure_kind: telos_agent::ToolFailureKind::ExecutionError,
            occurrences: 3,
            first_seen_unix_secs: 10,
            last_seen_unix_secs: 30,
            argument_summary: "cargo test".into(),
            error_summary: "failed".into(),
            exit_code: Some(101),
        };

        let url = reporter.create_issue(&group).await.unwrap();
        assert_eq!(url.as_deref(), Some("https://github.com/future-re/telos-agent/issues/1"));
    }

    fn sample_event(timestamp: u64, signature: &str) -> telos_agent::ToolFailureEvent {
        telos_agent::ToolFailureEvent {
            timestamp_unix_secs: timestamp,
            session_id: "s".into(),
            turn_id: 1,
            tool_call_id: "call".into(),
            tool_name: "Bash".into(),
            failure_kind: telos_agent::ToolFailureKind::ExecutionError,
            argument_summary: "cargo test".into(),
            error_summary: "failed".into(),
            exit_code: Some(101),
            signature: signature.into(),
        }
    }
}
