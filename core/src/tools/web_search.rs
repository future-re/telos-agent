use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

/// Tool that searches the web via DuckDuckGo Lite (no API key required).
///
/// Returns a list of search results with titles, URLs, and snippets.
pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "WebSearch".into(),
            description: "Search the web via DuckDuckGo. Returns titles, URLs, and snippets."
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
Summarize findings and cite sources. Prefer WebFetch when you already know the exact URL.",
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
                message: "curl returned DuckDuckGo bot challenge page; automated search is blocked"
                    .into(),
            });
        }

        let results = parse_ddg_lite(&body);

        Ok(ToolOutput::json(json!({"results": results, "count": results.len()})))
    }
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
    let start = line.rfind('>')?;
    let after = &line[start + 1..];
    // If this line ends with </a>, strip it
    let text = if let Some(close) = after.find("</a") {
        after[..close].to_string()
    } else {
        after.to_string()
    };
    let text = text.trim();
    if text.is_empty() { None } else { Some(text.to_string()) }
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
}
