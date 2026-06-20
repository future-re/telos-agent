use std::path::PathBuf;

use base64::Engine;
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use url::Url;

use super::cdp::CdpClient;
use super::scripts::{
    BROWSER_ACTION_HELPERS, BROWSER_CLICK_SCRIPT, BROWSER_SELECT_SCRIPT, BROWSER_STATE_SCRIPT,
    BROWSER_TYPE_SCRIPT, PAGE_SUMMARY_SCRIPT,
};
use super::util::*;
use crate::error::AgentError;
use crate::tool::ToolContext;
use crate::tools::display_relative;
use crate::tools::domain_filter::domain_matches_any;

pub(super) struct BrowserSession {
    pub(super) id: String,
    process: Option<Child>,
    pub(super) port: u16,
    user_data_dir: PathBuf,
    pub(super) artifact_dir: PathBuf,
    ws_url: String,
    pub(super) allowed_domains: Vec<String>,
    pub(super) prohibited_domains: Vec<String>,
    pub(super) viewport: Viewport,
    cdp: CdpClient,
}

impl BrowserSession {
    pub(super) async fn start(
        id: &str,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<Self, AgentError> {
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

    pub(super) async fn navigate(
        &mut self,
        url: &str,
        context: &ToolContext,
    ) -> Result<Value, AgentError> {
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

    pub(super) async fn state(&mut self, context: &ToolContext) -> Result<Value, AgentError> {
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

    pub(super) async fn click(
        &mut self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<Value, AgentError> {
        emit_progress(context, "clicking browser element", selector_summary(arguments));
        self.evaluate_action(BROWSER_CLICK_SCRIPT, arguments).await
    }

    pub(super) async fn type_text(
        &mut self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<Value, AgentError> {
        emit_progress(context, "typing into browser element", selector_summary(arguments));
        self.evaluate_action(BROWSER_TYPE_SCRIPT, arguments).await
    }

    pub(super) async fn select(
        &mut self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<Value, AgentError> {
        emit_progress(context, "selecting browser option", selector_summary(arguments));
        self.evaluate_action(BROWSER_SELECT_SCRIPT, arguments).await
    }

    pub(super) async fn scroll(
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

    pub(super) async fn back(&mut self, context: &ToolContext) -> Result<Value, AgentError> {
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

    pub(super) async fn screenshot(&mut self, context: &ToolContext) -> Result<Value, AgentError> {
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

    pub(super) async fn close(&mut self) -> Result<Value, AgentError> {
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

#[derive(Debug, Clone, Copy)]
pub(super) struct Viewport {
    pub(super) width: u32,
    pub(super) height: u32,
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
