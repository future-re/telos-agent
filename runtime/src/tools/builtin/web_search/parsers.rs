use serde_json::{Value, json};

/// Detect DuckDuckGo Lite bot challenge / CAPTCHA pages.
pub(super) fn is_bot_challenge(html: &str) -> bool {
    html.contains("anomaly-modal") || html.contains("bots use DuckDuckGo")
}

pub(super) fn is_bing_challenge_or_non_result(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("captcha")
        || lower.contains("verify you are human")
        || lower.contains("unusual traffic")
        || lower.contains("id=\"challenge")
}

pub(super) fn url_encode(s: &str) -> String {
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

pub(super) fn parse_bing_rss_results(xml: &str) -> Vec<Value> {
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

pub(super) fn parse_bing_results(html: &str) -> Vec<Value> {
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
pub(super) fn parse_ddg_lite(html: &str) -> Vec<Value> {
    let mut results = Vec::new();
    for line in html.lines() {
        if results.len() >= 10 {
            break;
        }
        if !line.contains("rel=\"nofollow\"") {
            continue;
        }

        let Some(href) = extract_attribute(line, "href") else { continue };
        if !href.starts_with("http") || href.contains("duckduckgo.com") {
            continue;
        }

        let title = extract_link_text(line);
        let snippet = extract_snippet_after_link(html, line).unwrap_or_default();

        results.push(json!({
            "title": title.unwrap_or_else(|| "(untitled)".to_string()),
            "url": href,
            "snippet": snippet,
        }));
    }

    results
}

fn extract_attribute(line: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = line.find(&pattern)?;
    let after = &line[start + pattern.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

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

fn extract_snippet_after_link(html: &str, link_line: &str) -> Option<String> {
    let link_pos = html.find(link_line)?;
    let after = &html[link_pos + link_line.len()..];

    if let Some(br_pos) = after.find("<br") {
        let after_br = &after[br_pos + 4..];
        let snippet_end = after_br.find('<').unwrap_or(after_br.len());
        let snippet = after_br[..snippet_end].trim();
        if !snippet.is_empty() {
            return Some(snippet.to_string());
        }
    }

    None
}
