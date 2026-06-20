use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tokio::time::{Duration, sleep};
use url::Url;

use crate::error::AgentError;
use crate::tool::{PermissionDecision, ToolContext};
#[cfg(test)]
use crate::tools::domain_filter::domain_matches_any;
use crate::tools::domain_filter::parse_domain_list;

pub(super) fn browser_session_key(arguments: &Value, context: &ToolContext) -> String {
    arguments
        .get("browser_session_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| context.session_id.clone())
}

pub(super) fn selector_schema(extra: Value) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert("browser_session_id".into(), json!({ "type": "string" }));
    properties.insert("element_id".into(), json!({ "type": "string" }));
    properties.insert("selector".into(), json!({ "type": "string" }));
    properties.insert("text".into(), json!({ "type": "string" }));
    properties.insert("sensitive".into(), json!({ "type": "boolean" }));
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            properties.insert(key.clone(), value.clone());
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "anyOf": [
            { "required": ["element_id"] },
            { "required": ["selector"] },
            { "required": ["text"] }
        ]
    })
}

pub(super) fn sensitive_action_permission(
    action: &str,
    arguments: &Value,
) -> Result<PermissionDecision, AgentError> {
    if arguments.get("sensitive").and_then(Value::as_bool) == Some(true) {
        return Ok(PermissionDecision::Ask { reason: format!("{action} marked sensitive") });
    }
    let mut text = String::new();
    for key in ["element_id", "selector", "text", "value"] {
        if let Some(value) = arguments.get(key).and_then(Value::as_str) {
            text.push_str(value);
            text.push(' ');
        }
    }
    let lower = text.to_lowercase();
    let sensitive_terms = [
        "delete", "remove", "submit", "publish", "send", "pay", "purchase", "checkout", "login",
        "sign in", "password", "token", "secret", "删除", "移除", "提交", "发布", "发送", "支付",
        "购买", "结账", "登录", "密码", "密钥",
    ];
    if sensitive_terms.iter().any(|term| lower.contains(term)) {
        return Ok(PermissionDecision::Ask {
            reason: format!("{action} may trigger a sensitive page action"),
        });
    }
    Ok(PermissionDecision::Allow)
}

pub(super) fn find_browser_path(context: &ToolContext) -> Result<PathBuf, AgentError> {
    if let Some(path) = context.env.get("TELOS_CHROME_PATH").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    for candidate in [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "microsoft-edge",
        "msedge",
        "msedge.exe",
    ] {
        if command_exists(candidate) {
            return Ok(PathBuf::from(candidate));
        }
    }
    for candidate in windows_edge_candidates() {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(AgentError::Config(
        "no Chromium-compatible browser found; install Chromium/Chrome/Edge or set TELOS_CHROME_PATH. In WSL, set TELOS_CHROME_PATH to msedge.exe or the Windows Edge msedge.exe path."
            .into(),
    ))
}

fn command_exists(command: &str) -> bool {
    std::process::Command::new(command)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn windows_edge_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/mnt/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"),
        PathBuf::from("/mnt/c/Program Files/Microsoft/Edge/Application/msedge.exe"),
    ]
}

pub(super) fn browser_arg_path(browser_path: &Path, path: &Path) -> String {
    if is_windows_browser_path(browser_path)
        && let Ok(output) = std::process::Command::new("wslpath").arg("-w").arg(path).output()
        && output.status.success()
    {
        let converted = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !converted.is_empty() {
            return converted;
        }
    }
    path.display().to_string()
}

fn is_windows_browser_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("msedge.exe") || name.ends_with(".exe"))
        .unwrap_or(false)
}

pub(super) fn reserve_local_port() -> Result<u16, AgentError> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|err| {
        AgentError::ToolExecution { tool: "BrowserStart".into(), message: err.to_string() }
    })?;
    let port = listener
        .local_addr()
        .map_err(|err| AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: err.to_string(),
        })?
        .port();
    drop(listener);
    Ok(port)
}

pub(super) async fn wait_for_cdp(port: u16) -> Result<(), AgentError> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}/json/version");
    for _ in 0..80 {
        if let Ok(response) = client.get(&url).send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err(AgentError::ToolExecution {
        tool: "BrowserStart".into(),
        message: "timed out waiting for Chromium DevTools endpoint".into(),
    })
}

pub(super) async fn create_page(port: u16) -> Result<String, AgentError> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}/json/new?about:blank");
    let response = match client.put(&url).send().await {
        Ok(response) if response.status().is_success() => response,
        _ => client.get(&url).send().await.map_err(|err| AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: format!("failed to create browser target: {err}"),
        })?,
    };
    if !response.status().is_success() {
        return Err(AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: format!("failed to create browser target: HTTP {}", response.status()),
        });
    }
    let target: Value = response.json().await.map_err(|err| AgentError::ToolExecution {
        tool: "BrowserStart".into(),
        message: format!("failed to parse browser target response: {err}"),
    })?;
    target.get("webSocketDebuggerUrl").and_then(Value::as_str).map(str::to_string).ok_or_else(
        || AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: "browser target did not include webSocketDebuggerUrl".into(),
        },
    )
}

pub(super) fn validate_http_url(url: &str) -> Result<(), AgentError> {
    let parsed = Url::parse(url).map_err(|err| AgentError::Validation(err.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(AgentError::Validation(format!(
            "Browser tools only support http/https URLs, got `{scheme}`"
        ))),
    }
}

pub(super) fn optional_bool(arguments: &Value, key: &str) -> Option<bool> {
    arguments.get(key).and_then(Value::as_bool)
}

pub(super) fn optional_u32(arguments: &Value, key: &str) -> Option<u32> {
    arguments.get(key).and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok())
}

pub(super) fn optional_i64(arguments: &Value, key: &str) -> Option<i64> {
    arguments.get(key).and_then(Value::as_i64)
}

pub(super) fn optional_string_array(
    arguments: &Value,
    key: &str,
) -> Result<Option<Vec<String>>, AgentError> {
    if arguments.get(key).is_none() {
        return Ok(None);
    }
    parse_domain_list(arguments, key).map(Some)
}

pub(super) fn parse_runtime_json_value(result: Value) -> Result<Value, AgentError> {
    if let Some(exception) = result.get("exceptionDetails") {
        return Err(AgentError::ToolExecution {
            tool: "Browser".into(),
            message: format!("browser JavaScript failed: {exception}"),
        });
    }
    let value = result.get("result").and_then(|result| result.get("value")).ok_or_else(|| {
        AgentError::ToolExecution {
            tool: "Browser".into(),
            message: "browser JavaScript did not return a value".into(),
        }
    })?;
    if let Some(text) = value.as_str() {
        serde_json::from_str(text).map_err(|err| AgentError::ToolExecution {
            tool: "Browser".into(),
            message: format!("browser JavaScript returned invalid JSON: {err}"),
        })
    } else {
        Ok(value.clone())
    }
}

pub(super) fn selector_summary(arguments: &Value) -> Value {
    json!({
        "element_id": arguments.get("element_id").and_then(Value::as_str),
        "selector": arguments.get("selector").and_then(Value::as_str),
        "text": arguments.get("text").and_then(Value::as_str).map(|text| text.chars().take(80).collect::<String>()),
    })
}

pub(super) fn emit_progress(context: &ToolContext, message: &str, data: Value) {
    if let Some(tx) = &context.progress {
        let _ = tx.send(crate::tool::ToolProgress {
            tool_call_id: None,
            message: message.to_string(),
            data: Some(data),
        });
    }
}

pub(super) fn browser_io_error(err: std::io::Error) -> AgentError {
    AgentError::ToolExecution { tool: "Browser".into(), message: err.to_string() }
}

pub(super) fn safe_path_segment(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .collect::<String>();
    if sanitized.is_empty() { "default".into() } else { sanitized }
}

pub(super) fn now_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

pub(super) fn candidate_bookmark_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        paths.push(home.join(".config/google-chrome/Default/Bookmarks"));
        paths.push(home.join(".config/chromium/Default/Bookmarks"));
        paths.push(home.join(".config/microsoft-edge/Default/Bookmarks"));
    }
    paths
}

pub(super) fn collect_bookmark_matches(
    content: &str,
    query: &str,
    limit: usize,
    results: &mut Vec<Value>,
) {
    let Ok(json) = serde_json::from_str::<Value>(content) else {
        return;
    };
    let query = query.to_lowercase();
    visit_bookmark_node(&json, &query, limit, results);
}

fn visit_bookmark_node(node: &Value, query: &str, limit: usize, results: &mut Vec<Value>) {
    if results.len() >= limit {
        return;
    }
    if let Some(obj) = node.as_object() {
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
        let url = obj.get("url").and_then(Value::as_str).unwrap_or("");
        let haystack = format!("{name} {url}").to_lowercase();
        if !url.is_empty() && haystack.contains(query) {
            results.push(json!({ "title": name, "url": url, "source": "bookmark" }));
        }
        if let Some(children) = obj.get("children").and_then(Value::as_array) {
            for child in children {
                visit_bookmark_node(child, query, limit, results);
            }
        }
        for key in ["roots", "bookmark_bar", "other", "synced"] {
            if let Some(child) = obj.get(key) {
                visit_bookmark_node(child, query, limit, results);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_matching_accepts_subdomains() {
        assert!(domain_matches_any("docs.example.com", &["example.com".into()]));
        assert!(domain_matches_any("example.com", &["example.com".into()]));
        assert!(!domain_matches_any("badexample.com", &["example.com".into()]));
    }

    #[test]
    fn sensitive_permission_flags_risky_words() {
        let decision =
            sensitive_action_permission("browser click", &json!({ "text": "Submit payment" }))
                .unwrap();
        assert!(matches!(decision, PermissionDecision::Ask { .. }));
    }

    #[test]
    fn safe_path_segment_replaces_unsafe_chars() {
        assert_eq!(safe_path_segment("session/one:two"), "session_one_two");
    }

    #[test]
    fn validates_http_urls_only() {
        assert!(validate_http_url("https://example.com").is_ok());
        assert!(validate_http_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn detects_windows_edge_executables() {
        assert!(is_windows_browser_path(Path::new("msedge.exe")));
        assert!(is_windows_browser_path(Path::new(
            "/mnt/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"
        )));
        assert!(!is_windows_browser_path(Path::new("microsoft-edge")));
    }
}
