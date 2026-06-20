#[tokio::test]
async fn web_fetch_tool_allows_loopback_http_test_servers() {
    use std::sync::Arc;
    use telos_agent::tool::{Tool, ToolContext};
    use telos_agent::tools::WebFetchTool;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/page"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<html><body><h1>Local Fixture</h1><script>ignored()</script></body></html>",
        ))
        .mount(&server)
        .await;

    let tool = WebFetchTool::new();
    let ctx = ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        tool_call_id: None,
        cwd: std::env::current_dir().unwrap(),
        env: Default::default(),
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    };

    let result =
        tool.invoke(serde_json::json!({"url": server.uri() + "/page"}), ctx).await.unwrap();
    let text = result.content["text"].as_str().unwrap();
    assert_eq!(text, "Local Fixture");
}

#[tokio::test]
async fn web_fetch_tool_returns_html_as_text() {
    use std::sync::Arc;
    use telos_agent::tool::{Tool, ToolContext};
    use telos_agent::tools::WebFetchTool;

    let tool = WebFetchTool::new();
    let ctx = ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        tool_call_id: None,
        cwd: std::env::current_dir().unwrap(),
        env: Default::default(),
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    };

    let result = tool.invoke(serde_json::json!({"url": "https://example.com"}), ctx).await.unwrap();
    let text = result.content["text"].as_str().unwrap();
    assert!(!text.is_empty());
    assert!(text.contains("Example Domain"), "text: {text}");
}

#[tokio::test]
async fn web_search_tool_returns_results() {
    use std::sync::Arc;
    use telos_agent::tool::{Tool, ToolContext};
    use telos_agent::tools::WebSearchTool;

    let tool = WebSearchTool;
    let ctx = ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        tool_call_id: None,
        cwd: std::env::current_dir().unwrap(),
        env: Default::default(),
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    };

    let result = tool.invoke(serde_json::json!({"query": "rust programming"}), ctx).await;
    match result {
        Ok(output) => {
            // Network succeeded — verify result structure
            let count = output.content["count"].as_u64().unwrap_or(0);
            assert!(count > 0, "expected at least one search result, got {count}");
        }
        Err(e) => {
            // Network failures (timeout, DNS, etc.) are acceptable in CI/test
            let msg = e.to_string();
            assert!(
                msg.contains("HTTP request")
                    || msg.contains("Bing search provider")
                    || msg.contains("DuckDuckGo")
                    || msg.contains("fallback failed"),
                "WebSearch tool returned unexpected error: {msg}"
            );
        }
    }
}

#[tokio::test]
async fn ask_user_question_validates_and_returns_questions() {
    use std::sync::Arc;
    use telos_agent::tool::{Tool, ToolContext};
    use telos_agent::tools::AskUserQuestionTool;

    let tool = AskUserQuestionTool;
    let ctx = ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        tool_call_id: None,
        cwd: std::env::current_dir().unwrap(),
        env: Default::default(),
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    };

    let result = tool
        .invoke(
            serde_json::json!({
                "questions": [{
                    "question": "What is your preference?",
                    "header": "Preference",
                    "options": [
                        {"label": "A", "description": "Option A description"},
                        {"label": "B", "description": "Option B description"}
                    ],
                    "multiSelect": false
                }]
            }),
            ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.content["status"].as_str().unwrap(), "questions_ready");
    assert!(result.content["questions"].as_array().unwrap().len() == 1);
}

#[tokio::test]
async fn ask_user_question_rejects_empty_questions() {
    use std::sync::Arc;
    use telos_agent::tool::{Tool, ToolContext};
    use telos_agent::tools::AskUserQuestionTool;

    let tool = AskUserQuestionTool;
    let ctx = ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        tool_call_id: None,
        cwd: std::env::current_dir().unwrap(),
        env: Default::default(),
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    };

    let result = tool.invoke(serde_json::json!({"questions": []}), ctx).await;
    assert!(result.is_err());
}
