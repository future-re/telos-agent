use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

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
            input_schema: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
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
If WebSearch fails because DuckDuckGo reports a bot challenge or blocked automated search, do not retry WebSearch in the same turn; \
switch to WebFetch with known official/source URLs, use available context, or ask the user for a source/search provider.",
        )
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::Validation("missing query".into()))?;

        match bing_cn_search(query) {
            Ok(output) => Ok(output),
            Err(bing_err) => match duckduckgo_lite_search(query) {
                Ok(output) => Ok(output),
                Err(ddg_err) => Err(AgentError::ToolExecution {
                    tool: "WebSearch".into(),
                    message: format!("{bing_err}; fallback failed: {ddg_err}"),
                }),
            },
        }
    }
}

fn bing_cn_search(query: &str) -> Result<ToolOutput, AgentError> {
    let encoded = url_encode(query);
    let url = format!("https://cn.bing.com/search?q={encoded}");

    let output = std::process::Command::new("curl")
        .args(["-sL", "--max-time", "15", "-H", "Accept-Language: zh-CN,zh;q=0.9,en;q=0.8", &url])
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
    let results = parse_bing_results(&body);
    if results.is_empty() {
        return Err(AgentError::ToolExecution {
            tool: "WebSearch".into(),
            message: "Bing China search returned no parseable results".into(),
        });
    }

    Ok(ToolOutput::json(json!({"provider": "bing_cn", "results": results, "count": results.len()})))
}

fn duckduckgo_lite_search(query: &str) -> Result<ToolOutput, AgentError> {
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

    let results = parse_ddg_lite(&body);

    Ok(ToolOutput::json(
        json!({"provider": "duckduckgo_lite", "results": results, "count": results.len()}),
    ))
}

/// Detect DuckDuckGo Lite bot challenge / CAPTCHA pages.
fn is_bot_challenge(html: &str) -> bool {
    html.contains("anomaly-modal") || html.contains("bots use DuckDuckGo")
}

fn url_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "+".to_string(),
            c if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' => {
                c.to_string()
            }
            c => format!("%{:02X}", c as u8),
        })
        .collect()
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
}
