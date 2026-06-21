use std::any::Any;

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::tui::theme::Theme;

mod agent;
mod status;
mod thinking;
mod tool_call;
mod user;

pub use crate::tui::tool_rendering::ToolState;
pub use agent::AgentCell;
pub use status::{ErrorCell, SeparatorCell, TurnSummaryCell};
pub use thinking::ThinkingCell;
pub use tool_call::ToolCallCell;
pub use user::UserCell;

/// A single entry in the chat conversation history.
///
/// Each variant knows how to render itself into a ratatui [`Frame`].
///
/// # Send requirement
/// Cells flow through `mpsc` channels so they must be `Send`.
pub trait HistoryCell: Send {
    /// Number of terminal lines this cell occupies at the given width.
    fn needed_lines(&self, width: usize) -> u16;

    /// Render this cell into `area` of the provided `frame`.
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme);

    /// Render this cell after skipping `top_skip` terminal lines.
    fn render_scrolled(&self, frame: &mut Frame, area: Rect, theme: &Theme, top_skip: u16) {
        if top_skip == 0 {
            self.render(frame, area, theme);
        }
    }

    /// Whether this cell is still accumulating content (streaming).
    fn is_streaming(&self) -> bool {
        false
    }

    /// Append text to a streaming cell. No-op for non-streaming cells.
    fn push_text(&mut self, _text: &str) {}

    /// Mark this cell as no longer receiving streamed content.
    fn finish_streaming(&mut self) {}

    /// Whether this cell can be selected for keyboard actions.
    fn is_selectable(&self) -> bool {
        false
    }

    /// Render this cell as selected.
    fn set_selected(&mut self, _selected: bool) {}

    /// Optional tool_call_id for ToolCallCell lookups.
    fn tool_call_id(&self) -> Option<&str> {
        None
    }

    /// Downcast to &dyn Any for type-specific operations.
    fn as_any(&self) -> &dyn Any;

    /// Downcast to &mut dyn Any for type-specific operations.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
