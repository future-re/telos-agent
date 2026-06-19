//! Browser automation tools backed by Chrome DevTools Protocol.
//!
//! The tools use an isolated, managed Chromium profile by default. They expose a
//! small browser-use-style action model: start/navigate/state/click/type/select/
//! scroll/back/screenshot/close.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use url::Url;

use crate::error::AgentError;
use crate::tool::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

use super::display_relative;

type SharedSession = Arc<Mutex<BrowserSession>>;

#[derive(Clone, Default)]
pub struct BrowserManager {
    sessions: Arc<Mutex<HashMap<String, SharedSession>>>,
}

impl BrowserManager {
    pub fn new() -> Self {
        Self::default()
    }

    async fn get(&self, key: &str) -> Option<SharedSession> {
        self.sessions.lock().await.get(key).cloned()
    }

    async fn start_or_update(
        &self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<(String, SharedSession, bool), AgentError> {
        let key = browser_session_key(arguments, context);
        let allowed_domains = optional_string_array(arguments, "allowed_domains")?;
        let prohibited_domains = optional_string_array(arguments, "prohibited_domains")?;

        if let Some(session) = self.get(&key).await {
            {
                let mut session = session.lock().await;
                if let Some(domains) = allowed_domains {
                    session.allowed_domains = domains;
                }
                if let Some(domains) = prohibited_domains {
                    session.prohibited_domains = domains;
                }
            }
            return Ok((key, session, false));
        }

        let session = BrowserSession::start(&key, arguments, context).await?;
        let shared = Arc::new(Mutex::new(session));
        self.sessions.lock().await.insert(key.clone(), shared.clone());
        Ok((key, shared, true))
    }

    async fn remove(&self, key: &str) -> Option<SharedSession> {
        self.sessions.lock().await.remove(key)
    }
}

struct BrowserSession {
    id: String,
    process: Option<Child>,
    port: u16,
    user_data_dir: PathBuf,
    artifact_dir: PathBuf,
    ws_url: String,
    allowed_domains: Vec<String>,
    prohibited_domains: Vec<String>,
    viewport: Viewport,
    cdp: CdpClient,
}

impl BrowserSession {
    async fn start(id: &str, arguments: &Value, context: &ToolContext) -> Result<Self, AgentError> {
        let viewport = Viewport::from_arguments(arguments)?;
        let headless = optional_bool(arguments, "headless").unwrap_or(true);
        let allowed_domains =
            optional_string_array(arguments, "allowed_domains")?.unwrap_or_default();
        let prohibited_domains =
            optional_string_array(arguments, "prohibited_domains")?.unwrap_or_default();
        let browser_path = find_browser_path(context)?;
        let port = reserve_local_port()?;
        let artifact_dir = context.cwd.join(".telos").join("browser").join(safe_path_segment(id));
        let user_data_dir = artifact_dir.join("profile");
        let downloads_dir = artifact_dir.join("downloads");
        tokio::fs::create_dir_all(&downloads_dir).await.map_err(browser_io_error)?;

        let mut command = Command::new(&browser_path);
        command
            .arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", browser_arg_path(&browser_path, &user_data_dir)))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-background-networking")
            .arg("--disable-sync")
            .arg("--disable-extensions")
            .arg("--disable-popup-blocking")
            .arg(format!("--window-size={},{}", viewport.width, viewport.height))
            .arg("about:blank")
            .kill_on_drop(true);
        if headless {
            command.arg("--headless=new");
        }
        if optional_bool(arguments, "no_sandbox").unwrap_or(false) {
            command.arg("--no-sandbox");
        }

        emit_progress(context, "starting managed Chromium", json!({ "port": port }));
        let process = command.spawn().map_err(|err| AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: format!("failed to start Chromium from `{}`: {err}", browser_path.display()),
        })?;

        wait_for_cdp(port).await?;
        let ws_url = create_page(port).await?;
        let mut cdp = CdpClient::connect(&ws_url).await?;
        cdp.call("Page.enable", json!({})).await?;
        cdp.call("Runtime.enable", json!({})).await?;
        cdp.call("DOM.enable", json!({})).await?;
        cdp.call(
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": viewport.width,
                "height": viewport.height,
                "deviceScaleFactor": 1,
                "mobile": false
            }),
        )
        .await?;

        Ok(Self {
            id: id.to_string(),
            process: Some(process),
            port,
            user_data_dir,
            artifact_dir,
            ws_url,
            allowed_domains,
            prohibited_domains,
            viewport,
            cdp,
        })
    }

    async fn navigate(&mut self, url: &str, context: &ToolContext) -> Result<Value, AgentError> {
        validate_http_url(url)?;
        self.check_domain(url)?;
        emit_progress(context, "navigating browser", json!({ "url": url }));
        self.cdp.call("Page.navigate", json!({ "url": url })).await?;
        self.wait_ready().await?;
        self.page_summary().await
    }

    async fn wait_ready(&mut self) -> Result<(), AgentError> {
        let expression = r#"
            new Promise((resolve) => {
                if (document.readyState === 'complete' || document.readyState === 'interactive') {
                    resolve(document.readyState);
                    return;
                }
                const done = () => resolve(document.readyState);
                window.addEventListener('load', done, { once: true });
                setTimeout(() => resolve(document.readyState), 5000);
            })
        "#;
        let _ = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "awaitPromise": true,
                    "returnByValue": true,
                    "timeout": 6000
                }),
            )
            .await?;
        Ok(())
    }

    async fn page_summary(&mut self) -> Result<Value, AgentError> {
        let state = self.evaluate_json(PAGE_SUMMARY_SCRIPT).await?;
        Ok(json!({
            "browser_session_id": self.id,
            "url": state.get("url").cloned().unwrap_or(Value::Null),
            "title": state.get("title").cloned().unwrap_or(Value::Null),
            "ready_state": state.get("ready_state").cloned().unwrap_or(Value::Null),
            "text_preview": state.get("text_preview").cloned().unwrap_or(Value::Null),
            "viewport": {
                "width": self.viewport.width,
                "height": self.viewport.height
            },
            "allowed_domains": self.allowed_domains,
            "prohibited_domains": self.prohibited_domains,
        }))
    }

    async fn state(&mut self, context: &ToolContext) -> Result<Value, AgentError> {
        emit_progress(context, "extracting browser state", json!({}));
        let state = self.evaluate_json(BROWSER_STATE_SCRIPT).await?;
        Ok(json!({
            "browser_session_id": self.id,
            "url": state.get("url").cloned().unwrap_or(Value::Null),
            "title": state.get("title").cloned().unwrap_or(Value::Null),
            "ready_state": state.get("ready_state").cloned().unwrap_or(Value::Null),
            "scroll": state.get("scroll").cloned().unwrap_or(Value::Null),
            "viewport": state.get("viewport").cloned().unwrap_or(Value::Null),
            "text_preview": state.get("text_preview").cloned().unwrap_or(Value::Null),
            "elements": state.get("elements").cloned().unwrap_or_else(|| json!([])),
        }))
    }

    async fn click(
        &mut self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<Value, AgentError> {
        emit_progress(context, "clicking browser element", selector_summary(arguments));
        self.evaluate_action(BROWSER_CLICK_SCRIPT, arguments).await
    }

    async fn type_text(
        &mut self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<Value, AgentError> {
        emit_progress(context, "typing into browser element", selector_summary(arguments));
        self.evaluate_action(BROWSER_TYPE_SCRIPT, arguments).await
    }

    async fn select(
        &mut self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<Value, AgentError> {
        emit_progress(context, "selecting browser option", selector_summary(arguments));
        self.evaluate_action(BROWSER_SELECT_SCRIPT, arguments).await
    }

    async fn scroll(
        &mut self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<Value, AgentError> {
        let delta_y = optional_i64(arguments, "delta_y").unwrap_or(600);
        let delta_x = optional_i64(arguments, "delta_x").unwrap_or(0);
        emit_progress(
            context,
            "scrolling browser page",
            json!({ "delta_x": delta_x, "delta_y": delta_y }),
        );
        let result = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": format!("window.scrollBy({delta_x}, {delta_y}); JSON.stringify({{ x: window.scrollX, y: window.scrollY }})"),
                    "returnByValue": true
                }),
            )
            .await?;
        parse_runtime_json_value(result)
    }

    async fn back(&mut self, context: &ToolContext) -> Result<Value, AgentError> {
        emit_progress(context, "going back in browser history", json!({}));
        self.cdp
            .call(
                "Runtime.evaluate",
                json!({ "expression": "history.back()", "returnByValue": true }),
            )
            .await?;
        self.wait_ready().await?;
        self.page_summary().await
    }

    async fn screenshot(&mut self, context: &ToolContext) -> Result<Value, AgentError> {
        emit_progress(context, "capturing browser screenshot", json!({}));
        tokio::fs::create_dir_all(&self.artifact_dir).await.map_err(browser_io_error)?;
        let result = self
            .cdp
            .call("Page.captureScreenshot", json!({ "format": "png", "fromSurface": true }))
            .await?;
        let data = result.get("data").and_then(Value::as_str).ok_or_else(|| {
            AgentError::ToolExecution {
                tool: "BrowserScreenshot".into(),
                message: "CDP screenshot response did not include data".into(),
            }
        })?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|err| {
            AgentError::ToolExecution {
                tool: "BrowserScreenshot".into(),
                message: format!("failed to decode screenshot: {err}"),
            }
        })?;
        let path = self.artifact_dir.join(format!("screenshot-{}.png", now_millis()));
        tokio::fs::write(&path, bytes).await.map_err(browser_io_error)?;
        Ok(json!({
            "browser_session_id": self.id,
            "path": path,
            "relative_path": display_relative(&context.cwd, &path),
            "format": "png"
        }))
    }

    async fn close(&mut self) -> Result<Value, AgentError> {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill().await;
            let _ = process.wait().await;
        }
        Ok(json!({
            "browser_session_id": self.id,
            "closed": true,
            "port": self.port,
            "artifact_dir": self.artifact_dir,
            "profile_dir": self.user_data_dir,
            "ws_url": self.ws_url,
        }))
    }

    async fn evaluate_json(&mut self, expression: &str) -> Result<Value, AgentError> {
        let result = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true
                }),
            )
            .await?;
        parse_runtime_json_value(result)
    }

    async fn evaluate_action(
        &mut self,
        script: &str,
        arguments: &Value,
    ) -> Result<Value, AgentError> {
        let input = serde_json::to_string(arguments)
            .map_err(|err| AgentError::Validation(err.to_string()))?;
        let expression = format!(
            "(() => {{ {helpers} return ({script})({input}); }})()",
            helpers = BROWSER_ACTION_HELPERS
        );
        let result = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true
                }),
            )
            .await?;
        let parsed = parse_runtime_json_value(result)?;
        if parsed.get("ok").and_then(Value::as_bool) == Some(false) {
            return Err(AgentError::ToolExecution {
                tool: "Browser".into(),
                message: parsed
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("browser action failed")
                    .to_string(),
            });
        }
        Ok(parsed)
    }

    fn check_domain(&self, url: &str) -> Result<(), AgentError> {
        let parsed = Url::parse(url).map_err(|err| AgentError::Validation(err.to_string()))?;
        let host = parsed.host_str().unwrap_or_default();
        if domain_matches_any(host, &self.prohibited_domains) {
            return Err(AgentError::PermissionDenied(format!(
                "browser navigation to `{host}` is prohibited"
            )));
        }
        if !self.allowed_domains.is_empty() && !domain_matches_any(host, &self.allowed_domains) {
            return Err(AgentError::PermissionDenied(format!(
                "browser navigation to `{host}` is outside allowed_domains"
            )));
        }
        Ok(())
    }
}

struct CdpClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl CdpClient {
    async fn connect(ws_url: &str) -> Result<Self, AgentError> {
        let (ws, _) = connect_async(ws_url).await.map_err(|err| AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: format!("failed to connect CDP websocket: {err}"),
        })?;
        Ok(Self { ws, next_id: 1 })
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value, AgentError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({ "id": id, "method": method, "params": params });
        self.ws.send(Message::Text(request.to_string().into())).await.map_err(|err| {
            AgentError::ToolExecution {
                tool: "Browser".into(),
                message: format!("failed to send CDP command `{method}`: {err}"),
            }
        })?;

        while let Some(message) = self.ws.next().await {
            let message = message.map_err(|err| AgentError::ToolExecution {
                tool: "Browser".into(),
                message: format!("failed reading CDP response for `{method}`: {err}"),
            })?;
            let text = match message {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                Message::Close(_) => {
                    return Err(AgentError::ToolExecution {
                        tool: "Browser".into(),
                        message: "CDP websocket closed".into(),
                    });
                }
                _ => continue,
            };
            let value: Value =
                serde_json::from_str(&text).map_err(|err| AgentError::ToolExecution {
                    tool: "Browser".into(),
                    message: format!("invalid CDP JSON response: {err}"),
                })?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(AgentError::ToolExecution {
                    tool: "Browser".into(),
                    message: format!("CDP `{method}` failed: {error}"),
                });
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }

        Err(AgentError::ToolExecution {
            tool: "Browser".into(),
            message: format!("CDP websocket ended before `{method}` completed"),
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct Viewport {
    width: u32,
    height: u32,
}

impl Viewport {
    fn from_arguments(arguments: &Value) -> Result<Self, AgentError> {
        let width = optional_u32(arguments, "width").unwrap_or(1280);
        let height = optional_u32(arguments, "height").unwrap_or(900);
        if width < 320 || height < 240 {
            return Err(AgentError::Validation("viewport must be at least 320x240".into()));
        }
        Ok(Self { width, height })
    }
}

pub struct BrowserStartTool {
    manager: BrowserManager,
}

impl BrowserStartTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserStartTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserStart".into(),
            description: "Start or reuse an isolated managed Chromium browser session.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "browser_session_id": { "type": "string" },
                    "headless": { "type": "boolean", "default": true },
                    "width": { "type": "integer", "default": 1280 },
                    "height": { "type": "integer", "default": 900 },
                    "allowed_domains": { "type": "array", "items": { "type": "string" } },
                    "prohibited_domains": { "type": "array", "items": { "type": "string" } },
                    "no_sandbox": { "type": "boolean", "description": "Only set when Chromium cannot run in the current sandbox." }
                }
            }),
        }
    }

    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use BrowserStart when a task needs a dynamic page or full browser automation. \
The managed browser uses an isolated profile by default. Prefer allowed_domains for scoped tasks. \
Do not use browser automation to bypass CAPTCHA, bot checks, paywalls, or access controls.",
        )
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let (key, session, started) = self.manager.start_or_update(&arguments, &context).await?;
        let session = session.lock().await;
        Ok(ToolOutput::json(json!({
            "browser_session_id": key,
            "started": started,
            "port": session.port,
            "artifact_dir": display_relative(&context.cwd, &session.artifact_dir),
            "headless": optional_bool(&arguments, "headless").unwrap_or(true),
            "viewport": { "width": session.viewport.width, "height": session.viewport.height },
            "allowed_domains": session.allowed_domains,
            "prohibited_domains": session.prohibited_domains
        })))
    }
}

pub struct BrowserNavigateTool {
    manager: BrowserManager,
}

impl BrowserNavigateTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserNavigateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserNavigate".into(),
            description:
                "Navigate a browser session to an http/https URL and return a page summary.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "browser_session_id": { "type": "string" },
                    "headless": { "type": "boolean" },
                    "width": { "type": "integer" },
                    "height": { "type": "integer" },
                    "allowed_domains": { "type": "array", "items": { "type": "string" } },
                    "prohibited_domains": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["url"]
            }),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        let url = arguments
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::Validation("missing string `url`".into()))?;
        validate_http_url(url)
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let url = arguments.get("url").and_then(Value::as_str).unwrap();
        let (_, session, _) = self.manager.start_or_update(&arguments, &context).await?;
        let mut session = session.lock().await;
        let result = session.navigate(url, &context).await?;
        Ok(ToolOutput::json(result))
    }
}

pub struct BrowserStateTool {
    manager: BrowserManager,
}

impl BrowserStateTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserStateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserState".into(),
            description: "Return visible page text, scroll state, and indexed interactive elements for the browser session.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "browser_session_id": { "type": "string" } }
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = self.require_session(&key).await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.state(&context).await?))
    }
}

impl BrowserStateTool {
    async fn require_session(&self, key: &str) -> Result<SharedSession, AgentError> {
        self.manager.get(key).await.ok_or_else(|| AgentError::ToolExecution {
            tool: "BrowserState".into(),
            message: "no browser session found; call BrowserNavigate or BrowserStart first".into(),
        })
    }
}

pub struct BrowserClickTool {
    manager: BrowserManager,
}

impl BrowserClickTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserClickTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserClick".into(),
            description: "Click an indexed browser element. Prefer element_id from BrowserState."
                .into(),
            input_schema: selector_schema(json!({})),
        }
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        sensitive_action_permission("browser click", arguments)
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserClick").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.click(&arguments, &context).await?))
    }
}

pub struct BrowserTypeTool {
    manager: BrowserManager,
}

impl BrowserTypeTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserTypeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserType".into(),
            description:
                "Type text into an indexed input, textarea, or contenteditable browser element."
                    .into(),
            input_schema: selector_schema(json!({
                "text": { "type": "string" },
                "clear": { "type": "boolean", "default": true }
            })),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        if arguments.get("text").and_then(Value::as_str).is_none() {
            return Err(AgentError::Validation("missing string `text`".into()));
        }
        Ok(())
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        sensitive_action_permission("browser typing", arguments)
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserType").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.type_text(&arguments, &context).await?))
    }
}

pub struct BrowserSelectTool {
    manager: BrowserManager,
}

impl BrowserSelectTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserSelectTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserSelect".into(),
            description: "Select a value in an indexed browser select element.".into(),
            input_schema: selector_schema(json!({
                "value": { "type": "string" }
            })),
        }
    }

    async fn validate(&self, arguments: &Value, _context: &ToolContext) -> Result<(), AgentError> {
        if arguments.get("value").and_then(Value::as_str).is_none() {
            return Err(AgentError::Validation("missing string `value`".into()));
        }
        Ok(())
    }

    async fn check_permission(
        &self,
        arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        sensitive_action_permission("browser select", arguments)
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserSelect").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.select(&arguments, &context).await?))
    }
}

pub struct BrowserScrollTool {
    manager: BrowserManager,
}

impl BrowserScrollTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserScrollTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserScroll".into(),
            description: "Scroll the browser page by pixel deltas.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "browser_session_id": { "type": "string" },
                    "delta_x": { "type": "integer", "default": 0 },
                    "delta_y": { "type": "integer", "default": 600 }
                }
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserScroll").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.scroll(&arguments, &context).await?))
    }
}

pub struct BrowserBackTool {
    manager: BrowserManager,
}

impl BrowserBackTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserBackTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserBack".into(),
            description: "Go back in the browser session history and return a page summary.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "browser_session_id": { "type": "string" } }
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserBack").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.back(&context).await?))
    }
}

pub struct BrowserScreenshotTool {
    manager: BrowserManager,
}

impl BrowserScreenshotTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserScreenshot".into(),
            description:
                "Capture a PNG screenshot of the browser page and save it as a workspace artifact."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": { "browser_session_id": { "type": "string" } }
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let session = require_session(&self.manager, &key, "BrowserScreenshot").await?;
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.screenshot(&context).await?))
    }
}

pub struct BrowserCloseTool {
    manager: BrowserManager,
}

impl BrowserCloseTool {
    pub fn new(manager: BrowserManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserCloseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserClose".into(),
            description: "Close a managed browser session.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "browser_session_id": { "type": "string" } }
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let key = browser_session_key(&arguments, &context);
        let Some(session) = self.manager.remove(&key).await else {
            return Ok(ToolOutput::json(json!({
                "browser_session_id": key,
                "closed": false,
                "reason": "session not found"
            })));
        };
        let mut session = session.lock().await;
        Ok(ToolOutput::json(session.close().await?))
    }
}

pub struct BrowserFindUrlTool;

#[async_trait]
impl Tool for BrowserFindUrlTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "BrowserFindUrl".into(),
            description: "Search local browser bookmarks/history metadata for likely URLs. Requires explicit approval.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "default": 10 }
                },
                "required": ["query"]
            }),
        }
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Ask {
            reason: "reading local browser bookmarks/history metadata requires approval".into(),
        })
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        let query = arguments
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::Validation("missing string `query`".into()))?;
        let limit = optional_u32(&arguments, "limit").unwrap_or(10).min(50) as usize;
        let mut results = Vec::new();
        for path in candidate_bookmark_paths() {
            if results.len() >= limit {
                break;
            }
            let Ok(content) = tokio::fs::read_to_string(&path).await else {
                continue;
            };
            collect_bookmark_matches(&content, query, limit, &mut results);
        }
        Ok(ToolOutput::json(json!({
            "query": query,
            "count": results.len(),
            "results": results,
            "note": "Only bookmark metadata is read in v1; browser history databases are intentionally not opened yet."
        })))
    }
}

async fn require_session(
    manager: &BrowserManager,
    key: &str,
    tool: &str,
) -> Result<SharedSession, AgentError> {
    manager.get(key).await.ok_or_else(|| AgentError::ToolExecution {
        tool: tool.into(),
        message: "no browser session found; call BrowserNavigate or BrowserStart first".into(),
    })
}

fn browser_session_key(arguments: &Value, context: &ToolContext) -> String {
    arguments
        .get("browser_session_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| context.session_id.clone())
}

fn selector_schema(extra: Value) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert("browser_session_id".into(), json!({ "type": "string" }));
    properties.insert("element_id".into(), json!({ "type": "string" }));
    properties.insert("selector".into(), json!({ "type": "string" }));
    properties.insert("text".into(), json!({ "type": "string" }));
    properties.insert("sensitive".into(), json!({ "type": "boolean" }));
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            properties.insert(key.clone(), value.clone());
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "anyOf": [
            { "required": ["element_id"] },
            { "required": ["selector"] },
            { "required": ["text"] }
        ]
    })
}

fn sensitive_action_permission(
    action: &str,
    arguments: &Value,
) -> Result<PermissionDecision, AgentError> {
    if arguments.get("sensitive").and_then(Value::as_bool) == Some(true) {
        return Ok(PermissionDecision::Ask { reason: format!("{action} marked sensitive") });
    }
    let mut text = String::new();
    for key in ["element_id", "selector", "text", "value"] {
        if let Some(value) = arguments.get(key).and_then(Value::as_str) {
            text.push_str(value);
            text.push(' ');
        }
    }
    let lower = text.to_lowercase();
    let sensitive_terms = [
        "delete", "remove", "submit", "publish", "send", "pay", "purchase", "checkout", "login",
        "sign in", "password", "token", "secret", "删除", "移除", "提交", "发布", "发送", "支付",
        "购买", "结账", "登录", "密码", "密钥",
    ];
    if sensitive_terms.iter().any(|term| lower.contains(term)) {
        return Ok(PermissionDecision::Ask {
            reason: format!("{action} may trigger a sensitive page action"),
        });
    }
    Ok(PermissionDecision::Allow)
}

fn find_browser_path(context: &ToolContext) -> Result<PathBuf, AgentError> {
    if let Some(path) = context.env.get("TELOS_CHROME_PATH").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    for candidate in [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "microsoft-edge",
        "msedge",
        "msedge.exe",
    ] {
        if command_exists(candidate) {
            return Ok(PathBuf::from(candidate));
        }
    }
    for candidate in windows_edge_candidates() {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(AgentError::Config(
        "no Chromium-compatible browser found; install Chromium/Chrome/Edge or set TELOS_CHROME_PATH. In WSL, set TELOS_CHROME_PATH to msedge.exe or the Windows Edge msedge.exe path."
            .into(),
    ))
}

fn command_exists(command: &str) -> bool {
    std::process::Command::new(command)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn windows_edge_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/mnt/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"),
        PathBuf::from("/mnt/c/Program Files/Microsoft/Edge/Application/msedge.exe"),
    ]
}

fn browser_arg_path(browser_path: &Path, path: &Path) -> String {
    if is_windows_browser_path(browser_path)
        && let Ok(output) = std::process::Command::new("wslpath").arg("-w").arg(path).output()
        && output.status.success()
    {
        let converted = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !converted.is_empty() {
            return converted;
        }
    }
    path.display().to_string()
}

fn is_windows_browser_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("msedge.exe") || name.ends_with(".exe"))
        .unwrap_or(false)
}

fn reserve_local_port() -> Result<u16, AgentError> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|err| {
        AgentError::ToolExecution { tool: "BrowserStart".into(), message: err.to_string() }
    })?;
    let port = listener
        .local_addr()
        .map_err(|err| AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: err.to_string(),
        })?
        .port();
    drop(listener);
    Ok(port)
}

async fn wait_for_cdp(port: u16) -> Result<(), AgentError> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}/json/version");
    for _ in 0..80 {
        if let Ok(response) = client.get(&url).send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err(AgentError::ToolExecution {
        tool: "BrowserStart".into(),
        message: "timed out waiting for Chromium DevTools endpoint".into(),
    })
}

async fn create_page(port: u16) -> Result<String, AgentError> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}/json/new?about:blank");
    let response = match client.put(&url).send().await {
        Ok(response) if response.status().is_success() => response,
        _ => client.get(&url).send().await.map_err(|err| AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: format!("failed to create browser target: {err}"),
        })?,
    };
    if !response.status().is_success() {
        return Err(AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: format!("failed to create browser target: HTTP {}", response.status()),
        });
    }
    let target: Value = response.json().await.map_err(|err| AgentError::ToolExecution {
        tool: "BrowserStart".into(),
        message: format!("failed to parse browser target response: {err}"),
    })?;
    target.get("webSocketDebuggerUrl").and_then(Value::as_str).map(str::to_string).ok_or_else(
        || AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: "browser target did not include webSocketDebuggerUrl".into(),
        },
    )
}

fn validate_http_url(url: &str) -> Result<(), AgentError> {
    let parsed = Url::parse(url).map_err(|err| AgentError::Validation(err.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(AgentError::Validation(format!(
            "Browser tools only support http/https URLs, got `{scheme}`"
        ))),
    }
}

fn optional_bool(arguments: &Value, key: &str) -> Option<bool> {
    arguments.get(key).and_then(Value::as_bool)
}

fn optional_u32(arguments: &Value, key: &str) -> Option<u32> {
    arguments.get(key).and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok())
}

fn optional_i64(arguments: &Value, key: &str) -> Option<i64> {
    arguments.get(key).and_then(Value::as_i64)
}

fn optional_string_array(arguments: &Value, key: &str) -> Result<Option<Vec<String>>, AgentError> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    let Some(values) = value.as_array() else {
        return Err(AgentError::Validation(format!("`{key}` must be an array of strings")));
    };
    let mut out = Vec::new();
    for value in values {
        let Some(item) = value.as_str() else {
            return Err(AgentError::Validation(format!("`{key}` must be an array of strings")));
        };
        let item = item.trim().trim_start_matches('.').to_ascii_lowercase();
        if !item.is_empty() {
            out.push(item);
        }
    }
    Ok(Some(out))
}

fn domain_matches_any(host: &str, domains: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    domains.iter().any(|domain| {
        let domain = domain.trim_start_matches('.').to_ascii_lowercase();
        host == domain || host.ends_with(&format!(".{domain}"))
    })
}

fn parse_runtime_json_value(result: Value) -> Result<Value, AgentError> {
    if let Some(exception) = result.get("exceptionDetails") {
        return Err(AgentError::ToolExecution {
            tool: "Browser".into(),
            message: format!("browser JavaScript failed: {exception}"),
        });
    }
    let value = result.get("result").and_then(|result| result.get("value")).ok_or_else(|| {
        AgentError::ToolExecution {
            tool: "Browser".into(),
            message: "browser JavaScript did not return a value".into(),
        }
    })?;
    if let Some(text) = value.as_str() {
        serde_json::from_str(text).map_err(|err| AgentError::ToolExecution {
            tool: "Browser".into(),
            message: format!("browser JavaScript returned invalid JSON: {err}"),
        })
    } else {
        Ok(value.clone())
    }
}

fn selector_summary(arguments: &Value) -> Value {
    json!({
        "element_id": arguments.get("element_id").and_then(Value::as_str),
        "selector": arguments.get("selector").and_then(Value::as_str),
        "text": arguments.get("text").and_then(Value::as_str).map(|text| text.chars().take(80).collect::<String>()),
    })
}

fn emit_progress(context: &ToolContext, message: &str, data: Value) {
    if let Some(tx) = &context.progress {
        let _ = tx.send(crate::tool::ToolProgress {
            tool_call_id: None,
            message: message.to_string(),
            data: Some(data),
        });
    }
}

fn browser_io_error(err: std::io::Error) -> AgentError {
    AgentError::ToolExecution { tool: "Browser".into(), message: err.to_string() }
}

fn safe_path_segment(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .collect::<String>();
    if sanitized.is_empty() { "default".into() } else { sanitized }
}

fn now_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

fn candidate_bookmark_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        paths.push(home.join(".config/google-chrome/Default/Bookmarks"));
        paths.push(home.join(".config/chromium/Default/Bookmarks"));
        paths.push(home.join(".config/microsoft-edge/Default/Bookmarks"));
    }
    paths
}

fn collect_bookmark_matches(content: &str, query: &str, limit: usize, results: &mut Vec<Value>) {
    let Ok(json) = serde_json::from_str::<Value>(content) else {
        return;
    };
    let query = query.to_lowercase();
    visit_bookmark_node(&json, &query, limit, results);
}

fn visit_bookmark_node(node: &Value, query: &str, limit: usize, results: &mut Vec<Value>) {
    if results.len() >= limit {
        return;
    }
    if let Some(obj) = node.as_object() {
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
        let url = obj.get("url").and_then(Value::as_str).unwrap_or("");
        let haystack = format!("{name} {url}").to_lowercase();
        if !url.is_empty() && haystack.contains(query) {
            results.push(json!({ "title": name, "url": url, "source": "bookmark" }));
        }
        if let Some(children) = obj.get("children").and_then(Value::as_array) {
            for child in children {
                visit_bookmark_node(child, query, limit, results);
            }
        }
        for key in ["roots", "bookmark_bar", "other", "synced"] {
            if let Some(child) = obj.get(key) {
                visit_bookmark_node(child, query, limit, results);
            }
        }
    }
}

const PAGE_SUMMARY_SCRIPT: &str = r#"
(() => JSON.stringify({
  url: location.href,
  title: document.title,
  ready_state: document.readyState,
  text_preview: (document.body?.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 4000)
}))()
"#;

const BROWSER_STATE_SCRIPT: &str = r#"
(() => {
  const candidates = Array.from(document.querySelectorAll(
    'a,button,input,textarea,select,[role="button"],[role="link"],[onclick],[contenteditable="true"]'
  ));
  const visible = (el) => {
    const rect = el.getBoundingClientRect();
    const style = window.getComputedStyle(el);
    return rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
  };
  let index = 0;
  const elements = [];
  for (const el of candidates) {
    if (elements.length >= 120) break;
    if (!visible(el)) continue;
    index += 1;
    const id = `e${index}`;
    el.setAttribute('data-telos-id', id);
    const rect = el.getBoundingClientRect();
    const tag = el.tagName.toLowerCase();
    const inputType = tag === 'input' ? (el.getAttribute('type') || 'text').toLowerCase() : null;
    const safeValue = inputType && ['password', 'hidden'].includes(inputType) ? '' : (el.value || '');
    elements.push({
      element_id: id,
      tag,
      type: inputType,
      text: (el.innerText || el.getAttribute('aria-label') || el.getAttribute('title') || el.value || '').replace(/\s+/g, ' ').trim().slice(0, 300),
      placeholder: el.getAttribute('placeholder') || '',
      name: el.getAttribute('name') || '',
      href: el.href || '',
      value: safeValue.slice(0, 300),
      disabled: !!el.disabled || el.getAttribute('aria-disabled') === 'true',
      rect: { x: Math.round(rect.x), y: Math.round(rect.y), width: Math.round(rect.width), height: Math.round(rect.height) }
    });
  }
  return JSON.stringify({
    url: location.href,
    title: document.title,
    ready_state: document.readyState,
    scroll: { x: window.scrollX, y: window.scrollY, max_y: document.documentElement.scrollHeight - window.innerHeight },
    viewport: { width: window.innerWidth, height: window.innerHeight },
    text_preview: (document.body?.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 8000),
    elements
  });
})()
"#;

const BROWSER_CLICK_SCRIPT: &str = r#"
async (args) => {
  const el = findTelosElement(args);
  if (!el) return JSON.stringify({ ok: false, error: 'element not found' });
  el.scrollIntoView({ block: 'center', inline: 'center' });
  await new Promise(resolve => setTimeout(resolve, 60));
  el.click();
  return JSON.stringify({ ok: true, action: 'click', element: describeTelosElement(el) });
}
"#;

const BROWSER_TYPE_SCRIPT: &str = r#"
async (args) => {
  const el = findTelosElement(args);
  if (!el) return JSON.stringify({ ok: false, error: 'element not found' });
  const text = args.text ?? '';
  el.scrollIntoView({ block: 'center', inline: 'center' });
  el.focus();
  const clear = args.clear !== false;
  if (el.isContentEditable) {
    if (clear) el.textContent = '';
    el.textContent = (el.textContent || '') + text;
  } else {
    if (clear) el.value = '';
    el.value = (el.value || '') + text;
  }
  el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertText' }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return JSON.stringify({ ok: true, action: 'type', element: describeTelosElement(el), length: text.length });
}
"#;

const BROWSER_SELECT_SCRIPT: &str = r#"
async (args) => {
  const el = findTelosElement(args);
  if (!el) return JSON.stringify({ ok: false, error: 'element not found' });
  if (el.tagName.toLowerCase() !== 'select') return JSON.stringify({ ok: false, error: 'element is not a select' });
  el.scrollIntoView({ block: 'center', inline: 'center' });
  el.value = args.value ?? '';
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return JSON.stringify({ ok: true, action: 'select', element: describeTelosElement(el), value: el.value });
}
"#;

const BROWSER_ACTION_HELPERS: &str = r#"
function findTelosElement(args) {
  if (args.element_id) {
    const escaped = CSS.escape(args.element_id);
    const byId = document.querySelector(`[data-telos-id="${escaped}"]`);
    if (byId) return byId;
  }
  if (args.selector) {
    const bySelector = document.querySelector(args.selector);
    if (bySelector) return bySelector;
  }
  if (args.text) {
    const target = String(args.text).toLowerCase();
    const all = Array.from(document.querySelectorAll('a,button,input,textarea,select,[role="button"],[role="link"],[onclick],[contenteditable="true"]'));
    return all.find(el => ((el.innerText || el.value || el.getAttribute('aria-label') || el.getAttribute('title') || '').toLowerCase()).includes(target));
  }
  return null;
}
function describeTelosElement(el) {
  return {
    element_id: el.getAttribute('data-telos-id') || '',
    tag: el.tagName.toLowerCase(),
    text: (el.innerText || el.value || el.getAttribute('aria-label') || '').replace(/\s+/g, ' ').trim().slice(0, 200)
  };
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_matching_accepts_subdomains() {
        assert!(domain_matches_any("docs.example.com", &["example.com".into()]));
        assert!(domain_matches_any("example.com", &["example.com".into()]));
        assert!(!domain_matches_any("badexample.com", &["example.com".into()]));
    }

    #[test]
    fn sensitive_permission_flags_risky_words() {
        let decision =
            sensitive_action_permission("browser click", &json!({ "text": "Submit payment" }))
                .unwrap();
        assert!(matches!(decision, PermissionDecision::Ask { .. }));
    }

    #[test]
    fn safe_path_segment_replaces_unsafe_chars() {
        assert_eq!(safe_path_segment("session/one:two"), "session_one_two");
    }

    #[test]
    fn validates_http_urls_only() {
        assert!(validate_http_url("https://example.com").is_ok());
        assert!(validate_http_url("file:///etc/passwd").is_err());
    }
}
