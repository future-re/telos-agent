//! Bidirectional HTTP event channel for the agent session.
//!
//! The channel runs an embedded HTTP server that provides two endpoints:
//!
//! - `POST /inject` — External callers push events into the agent context.
//!   The event payload is injected as a system message into the conversation.
//! - `GET /events` — External callers subscribe to the agent's TurnEvent
//!   stream via Server-Sent Events (SSE).
//!
//! Topics use glob patterns for subscription matching (e.g. `"github.*"`,
//! `"ci.*"`, `"*"`).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_core::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing;

use crate::agent::turn::TurnEvent;
use crate::error::AgentError;
use crate::model::message::Message;

// ── configuration ───────────────────────────────────────────────────────────

/// Top-level configuration for the embedded HTTP event channel.
///
/// Add this to [`AgentConfig`](crate::AgentConfig) via the
/// [`event_channel`](crate::AgentConfig::event_channel) field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventChannelConfig {
    /// When `false` (default) the HTTP server is not started.
    pub enabled: bool,
    /// The socket address the HTTP server listens on.
    #[serde(default = "default_listen_addr")]
    pub listen: SocketAddr,
    /// Glob patterns matching topics the agent should accept. An incoming
    /// event whose `topic` matches at least one pattern is injected into the
    /// conversation context.
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
}

/// A single topic subscription pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    /// Glob pattern (e.g. `"github.*"`, `"ci.alerts"`). An empty string or
    /// `"*"` matches every topic.
    pub topic: String,
}

fn default_listen_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 9090))
}

impl Default for EventChannelConfig {
    fn default() -> Self {
        Self { enabled: false, listen: default_listen_addr(), subscriptions: Vec::new() }
    }
}

// ── external event ──────────────────────────────────────────────────────────

/// An event pushed from outside into the agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalEvent {
    /// Topic used for subscription matching.
    pub topic: String,
    /// Free-form payload text injected into the agent's context.
    pub payload: String,
}

// ── internal types ──────────────────────────────────────────────────────────

/// Shared state held by the axum router.
#[derive(Clone)]
struct AppState {
    inject_tx: mpsc::Sender<ExternalEvent>,
    broadcast_tx: broadcast::Sender<TurnEvent>,
}

#[derive(Deserialize)]
struct InjectBody {
    topic: String,
    payload: String,
}

#[derive(Serialize)]
struct InjectResponse {
    accepted: bool,
    topic: String,
    message: String,
}

// ── EventChannel ────────────────────────────────────────────────────────────

/// A running bidirectional HTTP event channel.
///
/// Created via [`EventChannel::start`] and stored in
/// [`AgentSession`](crate::AgentSession). The channel is torn down when
/// `EventChannel` is dropped (the server graceful-shutdown completes).
pub struct EventChannel {
    inject_rx: mpsc::Receiver<ExternalEvent>,
    broadcast_tx: broadcast::Sender<TurnEvent>,
    subscriptions: Vec<Subscription>,
    _server: tokio::task::JoinHandle<()>,
    _shutdown: oneshot::Sender<()>,
}

impl EventChannel {
    /// Bind the HTTP server on [`EventChannelConfig::listen`] and spawn the
    /// event-loop in a background tokio task. Returns `Ok(None)` when
    /// `config.enabled` is `false`, or an error if binding fails.
    ///
    /// This is a **synchronous** method — it uses a std `TcpListener` to bind,
    /// then converts the socket to a tokio listener.
    pub fn start(config: EventChannelConfig) -> Result<Option<Self>, AgentError> {
        if !config.enabled {
            return Ok(None);
        }

        let addr = config.listen;

        let std_listener = std::net::TcpListener::bind(addr)
            .map_err(|e| AgentError::Config(format!("EventChannel: cannot bind to {addr}: {e}")))?;
        std_listener
            .set_nonblocking(true)
            .map_err(|e| AgentError::Config(format!("EventChannel: set_nonblocking: {e}")))?;
        let listener = tokio::net::TcpListener::from_std(std_listener)
            .map_err(|e| AgentError::Config(format!("EventChannel: from_std: {e}")))?;

        let (inject_tx, inject_rx) = mpsc::channel(256);
        let (broadcast_tx, _) = broadcast::channel(256);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let app_state = Arc::new(AppState { inject_tx, broadcast_tx: broadcast_tx.clone() });

        let app = Router::new()
            .route("/health", get(handle_health))
            .route("/inject", post(handle_inject))
            .route("/events", get(handle_events))
            .with_state(app_state);

        let server = tokio::spawn(async move {
            tracing::info!("EventChannel HTTP server listening on {addr}");
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
        });

        tracing::info!("EventChannel started on {addr}");

        Ok(Some(Self {
            inject_rx,
            broadcast_tx,
            subscriptions: config.subscriptions,
            _server: server,
            _shutdown: shutdown_tx,
        }))
    }

    /// Publish a [`TurnEvent`] to all connected SSE clients.
    pub fn publish(&self, event: &TurnEvent) {
        let _ = self.broadcast_tx.send(event.clone());
    }

    /// Non-blocking drain of all external events whose topic matches a
    /// configured subscription pattern.
    pub fn try_drain_incoming(&mut self) -> Vec<ExternalEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.inject_rx.try_recv() {
            if self.topic_matches(&event.topic) {
                events.push(event);
            }
        }
        events
    }

    /// Convert a drained [`ExternalEvent`] into a system message ready for
    /// injection into the conversation.
    pub fn to_system_message(event: &ExternalEvent) -> Message {
        Message::system(format!(
            "<external-event topic=\"{}\">\n{}\n</external-event>",
            event.topic, event.payload
        ))
    }

    fn topic_matches(&self, topic: &str) -> bool {
        if self.subscriptions.is_empty() {
            return false;
        }
        self.subscriptions.iter().any(|sub| {
            // Treat "*" as catch-all.
            if sub.topic == "*" || sub.topic.is_empty() {
                return true;
            }
            glob::Pattern::new(&sub.topic).map(|pat| pat.matches(topic)).unwrap_or(false)
        })
    }
}

// ── HTTP handlers ───────────────────────────────────────────────────────────

async fn handle_health() -> &'static str {
    "ok"
}

async fn handle_inject(
    State(state): State<Arc<AppState>>,
    Json(body): Json<InjectBody>,
) -> Json<InjectResponse> {
    let event = ExternalEvent { topic: body.topic.clone(), payload: body.payload };

    match state.inject_tx.send(event).await {
        Ok(()) => Json(InjectResponse {
            accepted: true,
            topic: body.topic,
            message: "event injected into agent context".into(),
        }),
        Err(_) => Json(InjectResponse {
            accepted: false,
            topic: body.topic,
            message: "agent channel closed".into(),
        }),
    }
}

fn make_sse_stream(
    mut rx: broadcast::Receiver<TurnEvent>,
) -> impl Stream<Item = Result<SseEvent, std::convert::Infallible>> {
    async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    yield Ok(SseEvent::default().data(json));
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(n, "SSE client lagged; skipping events");
                    rx = rx.resubscribe();
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

async fn handle_events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    let rx = state.broadcast_tx.subscribe();
    Sse::new(make_sse_stream(rx)).keep_alive(KeepAlive::default())
}
