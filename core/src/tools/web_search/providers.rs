use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::ToolOutput;
use crate::tools::web_search::filters::{DomainFilters, filter_results};
use crate::tools::web_search::parsers::{
    is_bing_challenge_or_non_result, is_bot_challenge, parse_bing_results, parse_bing_rss_results,
    parse_ddg_lite, url_encode,
};

pub(super) async fn bing_cn_search(
    query: &str,
    filters: &DomainFilters,
) -> Result<ToolOutput, AgentError> {
    let encoded = url_encode(query);
    let rss_url = format!("https://www.bing.com/search?q={encoded}&format=rss");
    match bing_search_url(&rss_url, parse_bing_rss_results, "bing_rss", filters).await {
        Ok(output) => Ok(output),
        Err(rss_err) => {
            let html_url = format!("https://cn.bing.com/search?q={encoded}");
            bing_search_url(&html_url, parse_bing_results, "bing_cn_html", filters).await.map_err(
                |html_err| AgentError::ToolExecution {
                    tool: "WebSearch".into(),
                    message: format!("{rss_err}; HTML fallback failed: {html_err}"),
                },
            )
        }
    }
}

async fn bing_search_url(
    url: &str,
    parser: fn(&str) -> Vec<Value>,
    provider: &str,
    filters: &DomainFilters,
) -> Result<ToolOutput, AgentError> {
    let body = fetch_search_body(url, Some("zh-CN,zh;q=0.9,en;q=0.8")).await?;
    if body.trim().is_empty() {
        return Err(AgentError::ToolExecution {
            tool: "WebSearch".into(),
            message: format!("Bing search provider `{provider}` returned an empty response body"),
        });
    }
    if is_bing_challenge_or_non_result(&body) {
        return Err(AgentError::ToolExecution {
            tool: "WebSearch".into(),
            message: format!(
                "Bing search provider `{provider}` returned a challenge or non-result page"
            ),
        });
    }

    let mut results = parser(&body);
    filter_results(&mut results, filters);
    if results.is_empty() {
        return Err(AgentError::ToolExecution {
            tool: "WebSearch".into(),
            message: format!("Bing search provider `{provider}` returned no parseable results"),
        });
    }

    Ok(ToolOutput::json(json!({
        "provider": provider,
        "results": results,
        "count": results.len(),
        "allowed_domains": filters.allowed_domains,
        "blocked_domains": filters.blocked_domains,
    })))
}

pub(super) async fn duckduckgo_lite_search(
    query: &str,
    filters: &DomainFilters,
) -> Result<ToolOutput, AgentError> {
    let encoded = url_encode(query);
    let ddg_url = format!("https://lite.duckduckgo.com/lite/?q={encoded}");

    let body = fetch_search_body(&ddg_url, None).await?;

    if is_bot_challenge(&body) {
        return Err(AgentError::ToolExecution {
                tool: "WebSearch".into(),
                message: "DuckDuckGo returned a bot challenge page; automated search is blocked. Do not retry WebSearch immediately; use WebFetch with known official/source URLs or ask the user for a source/search provider.".into(),
            });
    }

    let mut results = parse_ddg_lite(&body);
    filter_results(&mut results, filters);

    Ok(ToolOutput::json(json!({
        "provider": "duckduckgo_lite",
        "results": results,
        "count": results.len(),
        "allowed_domains": filters.allowed_domains,
        "blocked_domains": filters.blocked_domains,
    })))
}

async fn fetch_search_body(url: &str, accept_language: Option<&str>) -> Result<String, AgentError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("telos-agent/0.1")
        .build()
        .map_err(|err| AgentError::ToolExecution {
            tool: "WebSearch".into(),
            message: format!("failed to create HTTP client: {err}"),
        })?;
    let mut request = client.get(url);
    if let Some(value) = accept_language {
        request = request.header(reqwest::header::ACCEPT_LANGUAGE, value);
    }
    let response = request.send().await.map_err(|err| AgentError::ToolExecution {
        tool: "WebSearch".into(),
        message: format!("HTTP request failed: {err}"),
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(AgentError::ToolExecution {
            tool: "WebSearch".into(),
            message: format!("HTTP request returned status {status}"),
        });
    }
    response.text().await.map_err(|err| AgentError::ToolExecution {
        tool: "WebSearch".into(),
        message: format!("failed to read HTTP response body: {err}"),
    })
}
