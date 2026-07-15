use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::error::AgentError;

pub(super) struct CdpClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl CdpClient {
    pub(super) async fn connect(ws_url: &str) -> Result<Self, AgentError> {
        let (ws, _) = connect_async(ws_url).await.map_err(|err| AgentError::ToolExecution {
            tool: "BrowserStart".into(),
            message: format!("failed to connect CDP websocket: {err}"),
        })?;
        Ok(Self { ws, next_id: 1 })
    }

    pub(super) async fn call(&mut self, method: &str, params: Value) -> Result<Value, AgentError> {
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
