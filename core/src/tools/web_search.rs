use async_trait::async_trait;
use serde_json::{Value, json};
use url::Url;

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput, ToolProgress};

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

fn bing_cn_search(query: &str, filters: &DomainFilters) -> Result<ToolOutput, AgentError> {
    let encoded = url_encode(query);
    let rss_url = format!("https://www.bing.com/search?q={encoded}&format=rss");
    match bing_search_url(&rss_url, parse_bing_rss_results, "bing_rss", filters) {
        Ok(output) => Ok(output),
        Err(rss_err) => {
            let html_url = format!("https://cn.bing.com/search?q={encoded}");
            bing_search_url(&html_url, parse_bing_results, "bing_cn_html", filters).map_err(
                |html_err| AgentError::ToolExecution {
                    tool: "WebSearch".into(),
                    message: format!("{rss_err}; HTML fallback failed: {html_err}"),
                },
            )
        }
    }
}

fn bing_search_url(
    url: &str,
    parser: fn(&str) -> Vec<Value>,
    provider: &str,
    filters: &DomainFilters,
) -> Result<ToolOutput, AgentError> {
    let output = std::process::Command::new("curl")
        .args(["-sL", "--max-time", "15", "-H", "Accept-Language: zh-CN,zh;q=0.9,en;q=0.8", url])
        .output()
        .map_err(|e| AgentError::ToolExecution {
            tool: "WebSearch".into(),
            message: format!("failed to spawn curl for Bing China search: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AgentError::ToolExecution {
            tool: "WebSearch".into(),
            message: format!("Bing China search exited with {}: {stderr}", output.status),
        });
    }

    let body = String::from_utf8_lossy(&output.stdout);
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

fn duckduckgo_lite_search(query: &str, filters: &DomainFilters) -> Result<ToolOutput, AgentError> {
    let encoded = url_encode(query);
    let ddg_url = format!("https://lite.duckduckgo.com/lite/?q={encoded}");

    let output = std::process::Command::new("curl")
        .args(["-sL", "--max-time", "15", &ddg_url])
        .output()
        .map_err(|e| AgentError::ToolExecution {
            tool: "WebSearch".into(),
            message: format!("failed to spawn curl: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AgentError::ToolExecution {
            tool: "WebSearch".into(),
            message: format!("curl exited with {}: {stderr}", output.status),
        });
    }

    let body = String::from_utf8_lossy(&output.stdout);

    if is_bot_challenge(&body) {
        return Err(AgentError::ToolExecution {
                tool: "WebSearch".into(),
                message: "curl returned DuckDuckGo bot challenge page; automated search is blocked. Do not retry WebSearch immediately; use WebFetch with known official/source URLs or ask the user for a source/search provider.".into(),
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

/// Detect DuckDuckGo Lite bot challenge / CAPTCHA pages.
fn is_bot_challenge(html: &str) -> bool {
    html.contains("anomaly-modal") || html.contains("bots use DuckDuckGo")
}

fn is_bing_challenge_or_non_result(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("captcha")
        || lower.contains("verify you are human")
        || lower.contains("unusual traffic")
        || lower.contains("id=\"challenge")
}

fn url_encode(s: &str) -> String {
    s.as_bytes()
        .iter()
        .map(|byte| match *byte {
            b' ' => "+".to_string(),
            b if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') => {
                char::from(b).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}

fn parse_bing_rss_results(xml: &str) -> Vec<Value> {
    let mut results = Vec::new();
    for item in xml.split("<item>").skip(1) {
        if results.len() >= 10 {
            break;
        }
        let item = item.split("</item>").next().unwrap_or(item);
        let title =
            extract_xml_tag(item, "title").map(|text| html_unescape(strip_tags(&text).trim()));
        let href = extract_xml_tag(item, "link").map(|text| html_unescape(text.trim()));
        let snippet = extract_xml_tag(item, "description")
            .map(|text| strip_tags(&text))
            .map(|text| html_unescape(text.trim()))
            .unwrap_or_default();
        let pub_date = extract_xml_tag(item, "pubDate").map(|text| html_unescape(text.trim()));

        let Some(href) = href else { continue };
        if !href.starts_with("http") {
            continue;
        }

        results.push(json!({
            "title": title.unwrap_or_else(|| "(untitled)".to_string()),
            "url": href,
            "snippet": snippet,
            "published": pub_date,
        }));
    }
    results
}

fn parse_bing_results(html: &str) -> Vec<Value> {
    let mut results = Vec::new();
    for segment in html.split("<li class=\"b_algo\"").skip(1) {
        if results.len() >= 10 {
            break;
        }
        let href = extract_attribute(segment, "href");
        let Some(href) = href else { continue };
        if !href.starts_with("http") || href.contains("bing.com") {
            continue;
        }

        let title = extract_between(segment, "<h2", "</h2>")
            .and_then(extract_link_text)
            .unwrap_or_else(|| "(untitled)".to_string());
        let snippet = extract_between(segment, "<p", "</p>")
            .map(strip_tags)
            .map(|text| html_unescape(text.trim()))
            .unwrap_or_default();

        results.push(json!({
            "title": html_unescape(&title),
            "url": href,
            "snippet": snippet,
        }));
    }
    results
}

/// Parse DuckDuckGo Lite HTML search results.
///
/// DDG Lite returns a simple HTML table. Result links are `<a rel="nofollow" href="...">`
/// followed by snippet text in sibling elements.
fn parse_ddg_lite(html: &str) -> Vec<Value> {
    let mut results = Vec::new();

    // Find all <a rel="nofollow" ...> links that point to real URLs (not DDG internal).
    // We iterate through lines and look for anchor tags.
    for line in html.lines() {
        if results.len() >= 10 {
            break;
        }

        // Look for <a rel="nofollow" href="...
        if !line.contains("rel=\"nofollow\"") {
            continue;
        }

        let href = extract_attribute(line, "href");
        let href = match href {
            Some(h) => h,
            None => continue,
        };

        // Skip DDG internal links
        if !href.starts_with("http") || href.contains("duckduckgo.com") {
            continue;
        }

        // Extract title text: text between > and </a>
        let title = extract_link_text(line);

        let snippet = extract_snippet_after_link(html, line);
        let snippet = snippet.unwrap_or_default();

        results.push(json!({
            "title": title.unwrap_or_else(|| "(untitled)".to_string()),
            "url": href,
            "snippet": snippet,
        }));
    }

    results
}

/// Extract an HTML attribute value by name.
fn extract_attribute(line: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = line.find(&pattern)?;
    let after = &line[start + pattern.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// Extract the visible text content of an `<a>...</a>` tag on a single line.
fn extract_link_text(line: &str) -> Option<String> {
    let text = if let Some(anchor_close) = line.find("</a") {
        let before_close = &line[..anchor_close];
        let start = before_close.rfind('>')?;
        before_close[start + 1..].to_string()
    } else {
        let start = line.rfind('>')?;
        line[start + 1..].to_string()
    };
    let text = html_unescape(strip_tags(&text).trim());
    if text.is_empty() { None } else { Some(text) }
}

fn extract_between<'a>(text: &'a str, start_pattern: &str, end_pattern: &str) -> Option<&'a str> {
    let start = text.find(start_pattern)?;
    let after = &text[start..];
    let tag_end = after.find('>')?;
    let body = &after[tag_end + 1..];
    let end = body.find(end_pattern)?;
    Some(&body[..end])
}

fn extract_xml_tag(text: &str, tag: &str) -> Option<String> {
    let start_pattern = format!("<{tag}>");
    let end_pattern = format!("</{tag}>");
    let start = text.find(&start_pattern)?;
    let after = &text[start + start_pattern.len()..];
    let end = after.find(&end_pattern)?;
    Some(after[..end].to_string())
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn html_unescape(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DomainFilters {
    allowed_domains: Vec<String>,
    blocked_domains: Vec<String>,
}

impl DomainFilters {
    fn from_args(args: &Value) -> Result<Self, AgentError> {
        Ok(Self {
            allowed_domains: parse_domain_list(args, "allowed_domains")?,
            blocked_domains: parse_domain_list(args, "blocked_domains")?,
        })
    }

    fn allows(&self, url: &str) -> bool {
        let Ok(parsed) = Url::parse(url) else {
            return false;
        };
        let Some(host) = parsed.host_str() else {
            return false;
        };
        if domain_matches_any(host, &self.blocked_domains) {
            return false;
        }
        self.allowed_domains.is_empty() || domain_matches_any(host, &self.allowed_domains)
    }
}

fn parse_domain_list(args: &Value, key: &str) -> Result<Vec<String>, AgentError> {
    let Some(value) = args.get(key) else {
        return Ok(Vec::new());
    };
    let Some(values) = value.as_array() else {
        return Err(AgentError::Validation(format!("`{key}` must be an array of strings")));
    };
    let mut domains = Vec::new();
    for value in values {
        let Some(domain) = value.as_str() else {
            return Err(AgentError::Validation(format!("`{key}` must be an array of strings")));
        };
        let domain = domain.trim().trim_start_matches('.').to_ascii_lowercase();
        if !domain.is_empty() {
            domains.push(domain);
        }
    }
    Ok(domains)
}

fn domain_matches_any(host: &str, domains: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    domains.iter().any(|domain| {
        let domain = domain.trim_start_matches('.').to_ascii_lowercase();
        host == domain || host.ends_with(&format!(".{domain}"))
    })
}

fn filter_results(results: &mut Vec<Value>, filters: &DomainFilters) {
    if filters.allowed_domains.is_empty() && filters.blocked_domains.is_empty() {
        return;
    }
    results.retain(|result| {
        result.get("url").and_then(Value::as_str).map(|url| filters.allows(url)).unwrap_or(false)
    });
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

/// Try to find a snippet after a result link.
/// In DDG Lite, snippets often appear in a `<td class="result-snippet">` or after a `<br>` tag.
fn extract_snippet_after_link(html: &str, link_line: &str) -> Option<String> {
    // Find the link line position in the full HTML
    let link_pos = html.find(link_line)?;
    let after = &html[link_pos + link_line.len()..];

    // Look for the next `<td` with a snippet, or look for text after <br>
    // DDG Lite format: after the link there's often a <br> then snippet text
    if let Some(br_pos) = after.find("<br") {
        let after_br = &after[br_pos + 4..];
        // Find the next < or end
        let snippet_end = after_br.find('<').unwrap_or(after_br.len());
        let snippet = after_br[..snippet_end].trim();
        if !snippet.is_empty() {
            return Some(snippet.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ddg_bot_challenge_page() {
        let challenge_html =
            r#"<div class="anomaly-modal__title">Unfortunately, bots use DuckDuckGo too.</div>"#;
        assert!(is_bot_challenge(challenge_html));
    }

    #[test]
    fn normal_html_is_not_a_bot_challenge() {
        let normal_html = r#"<a rel="nofollow" href="https://example.com">Example</a><td class="result-snippet">A snippet</td>"#;
        assert!(!is_bot_challenge(normal_html));
    }

    #[test]
    fn url_encode_uses_utf8_percent_encoding() {
        assert_eq!(
            url_encode("2026世界杯 比赛结果"),
            "2026%E4%B8%96%E7%95%8C%E6%9D%AF+%E6%AF%94%E8%B5%9B%E7%BB%93%E6%9E%9C"
        );
    }

    #[test]
    fn detects_bing_challenge_pages() {
        assert!(is_bing_challenge_or_non_result("Please verify you are human"));
        assert!(is_bing_challenge_or_non_result("<div id=\"challenge-form\">captcha</div>"));
        assert!(!is_bing_challenge_or_non_result("<rss><channel><item></item></channel></rss>"));
    }

    #[test]
    fn domain_filters_allow_subdomains_and_block_domains() {
        let filters =
            DomainFilters { allowed_domains: vec!["example.com".into()], blocked_domains: vec![] };
        assert!(filters.allows("https://docs.example.com/page"));
        assert!(!filters.allows("https://other.com/page"));

        let filters =
            DomainFilters { allowed_domains: vec![], blocked_domains: vec!["example.com".into()] };
        assert!(!filters.allows("https://docs.example.com/page"));
        assert!(filters.allows("https://other.com/page"));
    }

    #[test]
    fn filter_results_applies_domain_filters() {
        let filters =
            DomainFilters { allowed_domains: vec!["example.com".into()], blocked_domains: vec![] };
        let mut results = vec![
            json!({"title": "A", "url": "https://example.com/a"}),
            json!({"title": "B", "url": "https://blocked.test/b"}),
        ];

        filter_results(&mut results, &filters);
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

        let results = parse_bing_results(html);
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

        let results = parse_bing_rss_results(xml);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["title"], "Example & Test");
        assert_eq!(results[0]["url"], "https://example.com/page");
        assert_eq!(results[0]["snippet"], "Snippet & details");
        assert_eq!(results[0]["published"], "Fri, 19 Jun 2026 16:09:20 GMT");
    }
}
