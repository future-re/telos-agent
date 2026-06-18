use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::AgentError;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

/// Tool that fetches a URL and returns its content as plain text.
///
/// HTTP URLs are automatically upgraded to HTTPS. Cross-host redirects are
/// reported back to the caller. Results are cached per URL for 15 minutes.
pub struct WebFetchTool {
    cache: Mutex<HashMap<String, (String, Instant)>>,
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self { cache: Mutex::new(HashMap::new()) }
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "WebFetch".into(),
            description: "Fetch a URL and convert to text. HTTP upgraded to HTTPS. Results cached for 15 min per URL.".into(),
            input_schema: json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["web_fetch"]
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use WebFetch to retrieve a specific public URL. Only fetch public `http`/`https` URLs. \
Verify that returned content is relevant and trustworthy before acting on it.",
        )
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn invoke(&self, args: Value, _: ToolContext) -> Result<ToolOutput, AgentError> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::Validation("missing url".into()))?;

        // Check cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some((content, time)) = cache.get(url)
                && time.elapsed() < Duration::from_secs(900)
            {
                // 15 minutes
                return Ok(ToolOutput::text(format!("[cached]\n{content}")));
            }
        }

        // Upgrade HTTP to HTTPS
        let url = if url.starts_with("http://") {
            url.replacen("http://", "https://", 1)
        } else {
            url.to_string()
        };

        // Fetch using curl
        let output = std::process::Command::new("curl")
            .args(["-sL", "--max-time", "30", &url])
            .output()
            .map_err(|e| AgentError::ToolExecution {
                tool: "WebFetch".into(),
                message: format!("failed to spawn curl: {e}"),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AgentError::ToolExecution {
                tool: "WebFetch".into(),
                message: format!("curl exited with {}: {stderr}", output.status),
            });
        }

        let body = String::from_utf8_lossy(&output.stdout).to_string();
        let text = strip_html(&body);
        let truncated: String = text.chars().take(50000).collect();

        // Cache and return
        {
            self.cache.lock().unwrap().insert(url, (truncated.clone(), Instant::now()));
        }

        Ok(ToolOutput::text(truncated))
    }
}

fn strip_html(html: &str) -> String {
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut result = String::new();
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '<' {
            // Check for script/style tags to skip their content
            let tag_lower: String = chars
                .iter()
                .skip(i + 1)
                .take_while(|&&ch| ch != '>' && ch != ' ')
                .map(|&ch| ch.to_ascii_lowercase())
                .collect();
            if tag_lower == "script" {
                in_script = true;
            } else if tag_lower == "style" {
                in_style = true;
            } else if tag_lower.starts_with("/script") {
                in_script = false;
            } else if tag_lower.starts_with("/style") {
                in_style = false;
            }

            if !in_script && !in_style {
                // Track whether this is a block-level tag for spacing
                let lower = tag_lower.to_lowercase();
                if matches!(
                    lower.as_str(),
                    "br" | "p"
                        | "div"
                        | "tr"
                        | "li"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "blockquote"
                        | "hr"
                        | "pre"
                        | "/p"
                        | "/div"
                        | "/tr"
                        | "/li"
                        | "/h1"
                        | "/h2"
                        | "/h3"
                        | "/h4"
                        | "/h5"
                        | "/h6"
                        | "/blockquote"
                        | "/pre"
                        | "table"
                        | "/table"
                        | "/ol"
                        | "/ul"
                        | "ol"
                        | "ul"
                        | "/title"
                        | "title"
                ) && !result.is_empty()
                    && !result.ends_with('\n')
                {
                    result.push('\n');
                }
            }
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag && !in_script && !in_style {
            result.push(c);
        }
        i += 1;
    }

    // Collapse multiple whitespace characters into a single space
    let mut collapsed = String::with_capacity(result.len());
    let mut prev_space = false;
    for c in result.chars() {
        if c.is_whitespace() && c != '\n' {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else {
            collapsed.push(c);
            prev_space = false;
        }
    }

    // Trim each line
    collapsed
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
