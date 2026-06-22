//! Clean TUI v2 — built on Codex's rendering patterns.
//!
//! Architecture:
//! - [`render::Renderable`] — single rendering trait
//! - [`render::FlexRenderable`] — automatic vertical layout
//! - [`history_cell::HistoryCell`] — `display_lines()` + `Paragraph::line_count()`
//! - [`chat::ChatWidget`] — scrollable conversation viewport
//! - [`composer::Composer`] — input panel (tui-textarea backed)
//! - [`status::StatusBar`] — bottom status line
//! - [`approval::ApprovalOverlay`] — approval popup
//! - [`app::App`] — application state + event loop
//! - [`turn`] — agent turn execution bridge
//! - [`run`] — terminal event loop

pub mod app;
pub mod approval;
pub mod chat;
pub mod composer;
pub mod history_cell;
pub mod keymap;
pub mod markdown;
pub mod render;
pub mod run;
pub mod spawn;
pub mod status;
pub mod theme;
pub mod tool_rendering;
pub mod turn;
