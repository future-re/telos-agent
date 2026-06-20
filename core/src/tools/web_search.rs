use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput, ToolProgress};

mod filters;
mod parsers;
mod providers;

use filters::DomainFilters;
use providers::{bing_cn_search, duckduckgo_lite_search};

/// Tool that searches the web without an API key.
///
/// It tries Bing China first for better availability on China networks, then
/// falls back to DuckDuckGo Lite.
///
/// Returns a list of search results with titles, URLs, and snippets.
pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "WebSearch".into(),
            description:
                "Search the web without an API key. Tries Bing China first, then DuckDuckGo Lite. Returns titles, URLs, and snippets."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "allowed_domains": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Only include search results from these domains."
                    },
                    "blocked_domains": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Never include search results from these domains."
                    }
                },
                "required": ["query"]
            }),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["web_search"]
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use WebSearch when you need up-to-date information not present in the codebase or conversation. \
Prefer source-first browsing: use WebFetch directly when you know a likely official/source URL, and use WebSearch only when you need discovery. \
WebSearch does not use an API key; it tries Bing China first, then DuckDuckGo Lite. \
Use `allowed_domains` or `blocked_domains` when the task should be scoped to specific sources. \
If WebSearch fails because DuckDuckGo reports a bot challenge or blocked automated search, do not retry WebSearch in the same turn; \
switch to WebFetch with known official/source URLs, use available context, or ask the user for a source/search provider.",
        )
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        if arguments.get("query").and_then(|v| v.as_str()).is_none() {
            return Err(AgentError::Validation("missing query".into()));
        }
        if arguments.get("allowed_domains").is_some() && arguments.get("blocked_domains").is_some()
        {
            return Err(AgentError::Validation(
                "cannot specify both allowed_domains and blocked_domains".into(),
            ));
        }
        let _ = DomainFilters::from_args(arguments)?;
        Ok(())
    }

    async fn invoke(&self, args: Value, context: ToolContext) -> Result<ToolOutput, AgentError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::Validation("missing query".into()))?;
        let filters = DomainFilters::from_args(&args)?;
        emit_search_progress(&context, "searching web", query, None);

        match bing_cn_search(query, &filters) {
            Ok(output) => {
                emit_result_progress(&context, query, &output);
                Ok(output)
            }
            Err(bing_err) => match duckduckgo_lite_search(query, &filters) {
                Ok(output) => {
                    emit_result_progress(&context, query, &output);
                    Ok(output)
                }
                Err(ddg_err) => Err(AgentError::ToolExecution {
                    tool: "WebSearch".into(),
                    message: format!("{bing_err}; fallback failed: {ddg_err}"),
                }),
            },
        }
    }
}

fn emit_search_progress(
    context: &ToolContext,
    message: &str,
    query: &str,
    result_count: Option<usize>,
) {
    if let Some(tx) = &context.progress {
        let _ = tx.send(ToolProgress {
            tool_call_id: None,
            message: message.to_string(),
            data: Some(json!({
                "query": query,
                "result_count": result_count,
            })),
        });
    }
}

fn emit_result_progress(context: &ToolContext, query: &str, output: &ToolOutput) {
    let count = output.content.get("count").and_then(Value::as_u64).map(|count| count as usize);
    emit_search_progress(context, "search results received", query, count);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ddg_bot_challenge_page() {
        let challenge_html =
            r#"<div class="anomaly-modal__title">Unfortunately, bots use DuckDuckGo too.</div>"#;
        assert!(parsers::is_bot_challenge(challenge_html));
    }

    #[test]
    fn normal_html_is_not_a_bot_challenge() {
        let normal_html = r#"<a rel="nofollow" href="https://example.com">Example</a><td class="result-snippet">A snippet</td>"#;
        assert!(!parsers::is_bot_challenge(normal_html));
    }

    #[test]
    fn url_encode_uses_utf8_percent_encoding() {
        assert_eq!(
            parsers::url_encode("2026世界杯 比赛结果"),
            "2026%E4%B8%96%E7%95%8C%E6%9D%AF+%E6%AF%94%E8%B5%9B%E7%BB%93%E6%9E%9C"
        );
    }

    #[test]
    fn detects_bing_challenge_pages() {
        assert!(parsers::is_bing_challenge_or_non_result("Please verify you are human"));
        assert!(parsers::is_bing_challenge_or_non_result(
            "<div id=\"challenge-form\">captcha</div>"
        ));
        assert!(!parsers::is_bing_challenge_or_non_result(
            "<rss><channel><item></item></channel></rss>"
        ));
    }

    #[test]
    fn domain_filters_allow_subdomains_and_block_domains() {
        let filters = filters::DomainFilters {
            allowed_domains: vec!["example.com".into()],
            blocked_domains: vec![],
        };
        assert!(filters.allows("https://docs.example.com/page"));
        assert!(!filters.allows("https://other.com/page"));

        let filters = filters::DomainFilters {
            allowed_domains: vec![],
            blocked_domains: vec!["example.com".into()],
        };
        assert!(!filters.allows("https://docs.example.com/page"));
        assert!(filters.allows("https://other.com/page"));
    }

    #[test]
    fn filter_results_applies_domain_filters() {
        let filters = filters::DomainFilters {
            allowed_domains: vec!["example.com".into()],
            blocked_domains: vec![],
        };
        let mut results = vec![
            json!({"title": "A", "url": "https://example.com/a"}),
            json!({"title": "B", "url": "https://blocked.test/b"}),
        ];

        filters::filter_results(&mut results, &filters);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["url"], "https://example.com/a");
    }

    #[test]
    fn prompt_discourages_retrying_after_bot_challenge() {
        let prompt = WebSearchTool.prompt_text().unwrap();
        assert!(prompt.contains("bot challenge"));
        assert!(prompt.contains("do not retry WebSearch"));
        assert!(prompt.contains("WebFetch"));
        assert!(prompt.contains("Bing China"));
        assert!(!prompt.contains("BRAVE_SEARCH_API_KEY"));
    }

    #[test]
    fn parses_bing_results() {
        let html = r#"
            <li class="b_algo">
              <h2><a href="https://example.com">Example &amp; Test</a></h2>
              <div class="b_caption"><p>Example snippet &amp; details</p></div>
            </li>
        "#;

        let results = parsers::parse_bing_results(html);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["title"], "Example & Test");
        assert_eq!(results[0]["url"], "https://example.com");
        assert_eq!(results[0]["snippet"], "Example snippet & details");
    }

    #[test]
    fn parses_bing_rss_results() {
        let xml = r#"
            <?xml version="1.0" encoding="utf-8" ?>
            <rss version="2.0">
              <channel>
                <item>
                  <title>Example &amp; Test</title>
                  <link>https://example.com/page</link>
                  <description>Snippet &amp; details</description>
                  <pubDate>Fri, 19 Jun 2026 16:09:20 GMT</pubDate>
                </item>
              </channel>
            </rss>
        "#;

        let results = parsers::parse_bing_rss_results(xml);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["title"], "Example & Test");
        assert_eq!(results[0]["url"], "https://example.com/page");
        assert_eq!(results[0]["snippet"], "Snippet & details");
        assert_eq!(results[0]["published"], "Fri, 19 Jun 2026 16:09:20 GMT");
    }
}
