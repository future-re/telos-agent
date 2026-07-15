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
        .redirect(reqwest::redirect::Policy::none())
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

    #[test]
    fn exec_http_hook_returns_error_status_body_like_curl_s() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/missing"))
                .respond_with(ResponseTemplate::new(404).set_body_string("not found body"))
                .mount(&server)
                .await;
            server
        });

        let response =
            exec_http_hook(&format!("{}/missing", server.uri()), "GET", &HashMap::new(), None)
                .unwrap();

        assert_eq!(response, "not found body");
    }

    #[test]
    fn exec_http_hook_does_not_follow_redirects_like_curl_s() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/redirect"))
                .respond_with(
                    ResponseTemplate::new(302)
                        .insert_header("location", "/target")
                        .set_body_string("redirect body"),
                )
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/target"))
                .respond_with(ResponseTemplate::new(200).set_body_string("followed body"))
                .mount(&server)
                .await;
            server
        });

        let response =
            exec_http_hook(&format!("{}/redirect", server.uri()), "GET", &HashMap::new(), None)
                .unwrap();

        assert_eq!(response, "redirect body");
    }
}
