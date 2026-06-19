use crossterm::event::{KeyEvent, MouseEvent};
use telos_agent::TurnEvent;

/// Events that flow through the TUI event loop.
#[derive(Debug, Clone)]
pub enum Event {
    /// A keyboard event from crossterm.
    Key(KeyEvent),
    /// A mouse event from crossterm.
    Mouse(MouseEvent),
    /// Terminal was resized.
    Resize { cols: u16, rows: u16 },
    /// A turn event from the agent stream.
    Turn(TurnEvent),
    /// The agent turn completed (stream ended).
    TurnComplete,
    /// A non-recoverable error from the session or turn stream.
    SessionError { message: String },
    /// Request to redraw (e.g. from a timer tick).
    Tick,
}

/// Internal application events for component-to-component communication.
///
/// Components that hold a clone of the [`UnboundedSender<AppEvent>`] can emit
/// these to update shared state without coupling directly to the App struct.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Update the status bar text.
    StatusChanged(String),
    /// Token usage update from the background task or runtime.
    TokenUsage { used: u64, max: u64 },
    /// A configuration value changed (e.g. auto-mode toggled).
    ConfigChanged(String),
}
