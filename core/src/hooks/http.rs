use crate::error::AgentError;
use std::collections::HashMap;

/// Execute an HTTP-type hook.
pub fn exec_http_hook(
    url: &str,
    method: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Result<String, AgentError> {
    let method = method.parse::<reqwest::Method>().map_err(|err| AgentError::ToolExecution {
        tool: "HttpHook".into(),
        message: format!("invalid HTTP method `{method}`: {err}"),
    })?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("telos-agent/0.1")
        .build()
        .map_err(|err| AgentError::ToolExecution {
            tool: "HttpHook".into(),
            message: format!("failed to create HTTP client: {err}"),
        })?;

    let mut request = client.request(method, url);
    for (k, v) in headers {
        request = request.header(k, v);
    }
    if let Some(body) = body {
        request = request.body(body.to_string());
    }

    let response = request.send().map_err(|err| AgentError::ToolExecution {
        tool: "HttpHook".into(),
        message: format!("HTTP request failed: {err}"),
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(AgentError::ToolExecution {
            tool: "HttpHook".into(),
            message: format!("HTTP request returned status {status}"),
        });
    }
    response.text().map_err(|err| AgentError::ToolExecution {
        tool: "HttpHook".into(),
        message: format!("failed to read HTTP response body: {err}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn exec_http_hook_posts_headers_and_body_without_curl() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/hook"))
                .and(header("x-test", "yes"))
                .and(body_string("payload"))
                .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
                .mount(&server)
                .await;
            server
        });
        let headers = HashMap::from([("x-test".to_string(), "yes".to_string())]);

        let response =
            exec_http_hook(&format!("{}/hook", server.uri()), "POST", &headers, Some("payload"))
                .unwrap();

        assert_eq!(response, "ok");
    }

    #[test]
    fn exec_http_hook_rejects_invalid_method() {
        let err = exec_http_hook("http://localhost", "not a method", &HashMap::new(), None)
            .unwrap_err()
            .to_string();

        assert!(err.contains("invalid HTTP method"), "{err}");
    }
}
