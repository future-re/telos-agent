//! Scrollable chat widget — the main conversation viewport.
//!
//! Cells are appended in chronological order. The viewport is bottom-anchored:
//! new content pushes up from the bottom. When the user scrolls up
//! (`user_scrolled_up = true`), auto-scroll is paused until they scroll
//! back to the bottom or press a "scroll to bottom" key.
//!
//! Rendering collects all cell lines into one `Paragraph` and scrolls
//! with `Paragraph::scroll()`. Height measurement uses
//! `Paragraph::line_count()` for accuracy.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use crate::tui::history_cell::HistoryCell;
use crate::tui::render::Renderable;

pub struct ChatWidget {
    /// All conversation cells in chronological order.
    cells: Vec<Box<dyn HistoryCell>>,
    /// Index of the last streaming cell.
    active_idx: Option<usize>,
    /// Index of the active assistant streaming cell.
    assistant_idx: Option<usize>,
    /// Index of the active thinking streaming cell.
    thinking_idx: Option<usize>,
    /// Currently selected cell for keyboard actions.
    selected_idx: Option<usize>,
    /// Scroll offset from bottom (0 = bottom).
    pub scroll_offset: usize,
    /// Whether the user has manually scrolled up.
    user_scrolled_up: bool,
}

impl ChatWidget {
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            active_idx: None,
            assistant_idx: None,
            thinking_idx: None,
            selected_idx: None,
            scroll_offset: 0,
            user_scrolled_up: false,
        }
    }

    // ── Mutation ──────────────────────────────────────────────────────────

    pub fn push_cell(&mut self, cell: Box<dyn HistoryCell>) {
        if cell.is_streaming() {
            self.active_idx = Some(self.cells.len());
        }
        self.cells.push(cell);
        self.maybe_scroll_to_bottom();
    }

    pub fn push_agent_delta(&mut self, text: &str) {
        if let Some(idx) = self.assistant_idx
            && self.cells.get(idx).is_some_and(|c| c.is_streaming())
        {
            self.cells[idx].push_delta(text);
            self.maybe_scroll_to_bottom();
            return;
        }
        let cell = Box::new(crate::tui::history_cell::AgentCell {
            buffer: text.to_string(),
            is_streaming: true,
        });
        let idx = self.cells.len();
        self.cells.push(cell);
        self.assistant_idx = Some(idx);
        self.active_idx = Some(idx);
        self.maybe_scroll_to_bottom();
    }

    pub fn push_thinking_delta(&mut self, text: &str) {
        if let Some(idx) = self.thinking_idx
            && self.cells.get(idx).is_some_and(|c| c.is_streaming())
        {
            self.cells[idx].push_delta(text);
            self.maybe_scroll_to_bottom();
            return;
        }
        let cell = Box::new(crate::tui::history_cell::ThinkingCell {
            buffer: text.to_string(),
            is_streaming: true,
        });
        let idx = self.cells.len();
        self.cells.push(cell);
        self.thinking_idx = Some(idx);
        self.active_idx = Some(idx);
        self.maybe_scroll_to_bottom();
    }

    pub fn finish_streaming(&mut self) {
        for idx in [self.assistant_idx.take(), self.thinking_idx.take()].into_iter().flatten() {
            if let Some(cell) = self.cells.get_mut(idx) {
                cell.finish();
            }
        }
        self.active_idx = None;
    }

    pub fn has_active_assistant(&self) -> bool {
        self.assistant_idx.is_some()
    }

    pub fn find_tool_call_mut(&mut self, id: &str) -> Option<&mut Box<dyn HistoryCell>> {
        self.cells.iter_mut().find(|c| c.tool_call_id() == Some(id))
    }

    // ── Selection ─────────────────────────────────────────────────────────

    pub fn select_next_tool(&mut self) {
        self.move_selection(1);
    }

    pub fn select_prev_tool(&mut self) {
        self.move_selection(-1);
    }

    pub fn toggle_selected(&mut self) -> bool {
        let Some(idx) = self.selected_idx else { return false };
        if let Some(cell) = self.cells.get_mut(idx) {
            // Toggle expand via downcast to ToolCallCell
            if let Some(tool) =
                cell.as_any_mut().downcast_mut::<crate::tui::history_cell::ToolCallCell>()
            {
                tool.expanded = !tool.expanded;
                self.maybe_scroll_to_bottom();
                return true;
            }
        }
        false
    }

    // ── Scrolling ─────────────────────────────────────────────────────────

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
        self.user_scrolled_up = true;
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        if self.scroll_offset == 0 {
            self.user_scrolled_up = false;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.user_scrolled_up = false;
    }

    fn maybe_scroll_to_bottom(&mut self) {
        if !self.user_scrolled_up {
            self.scroll_offset = 0;
        }
    }

    pub fn clear(&mut self) {
        self.cells.clear();
        self.active_idx = None;
        self.assistant_idx = None;
        self.thinking_idx = None;
        self.selected_idx = None;
        self.scroll_offset = 0;
        self.user_scrolled_up = false;
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    // ── Internals ─────────────────────────────────────────────────────────

    fn move_selection(&mut self, delta: isize) {
        let selectable: Vec<usize> = self
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_selectable())
            .map(|(i, _)| i)
            .collect();
        if selectable.is_empty() {
            return;
        }
        let pos = self.selected_idx.and_then(|idx| selectable.iter().position(|&c| c == idx));
        let next = match pos {
            Some(p) => (p as isize + delta).rem_euclid(selectable.len() as isize) as usize,
            None => {
                if delta < 0 {
                    selectable.len() - 1
                } else {
                    0
                }
            }
        };
        self.set_selected(Some(selectable[next]));
    }

    fn set_selected(&mut self, idx: Option<usize>) {
        if let Some(old) = self.selected_idx
            && let Some(cell) = self.cells.get_mut(old)
        {
            cell.set_selected(false);
        }
        self.selected_idx = idx;
        if let Some(new) = self.selected_idx
            && let Some(cell) = self.cells.get_mut(new)
        {
            cell.set_selected(true);
        }
    }
}

impl Renderable for ChatWidget {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if self.cells.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }

        // Collect all lines from all cells.
        let mut all_lines: Vec<Line<'static>> = Vec::new();
        for cell in &self.cells {
            all_lines.extend(cell.display_lines(area.width));
        }

        // Bottom-anchor: pad when content is shorter than viewport.
        let visible = area.height as usize;
        let total = all_lines.len();
        if total < visible {
            let pad = visible - total;
            let mut padded = vec![Line::from(""); pad];
            padded.append(&mut all_lines);
            all_lines = padded;
        }

        let total = all_lines.len();
        let max_scroll = total.saturating_sub(visible);
        // scroll_offset is "lines from bottom"; convert to paragraph scroll.
        let paragraph_scroll = max_scroll.saturating_sub(self.scroll_offset) as u16;

        let paragraph = Paragraph::new(Text::from(all_lines))
            .scroll((paragraph_scroll, 0))
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.cells.iter().map(|c| c.desired_height(width)).sum()
    }
}

impl Default for ChatWidget {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::history_cell::{AgentCell, UserCell};

    #[test]
    fn scroll_to_bottom_resets_user_scrolled() {
        let mut chat = ChatWidget::new();
        chat.push_cell(Box::new(UserCell { content: "hi".into() }));
        chat.scroll_up(5);
        assert!(chat.scroll_offset > 0);
        chat.scroll_to_bottom();
        assert_eq!(chat.scroll_offset, 0);
    }

    #[test]
    fn user_scrolled_up_prevents_auto_scroll() {
        let mut chat = ChatWidget::new();
        chat.push_cell(Box::new(UserCell { content: "first".into() }));
        chat.scroll_up(5);
        chat.push_cell(Box::new(AgentCell { buffer: "second".into(), is_streaming: false }));
        assert!(chat.scroll_offset > 0); // Not reset because user scrolled
    }

    #[test]
    fn streaming_deltas_append_to_active_cell() {
        let mut chat = ChatWidget::new();
        chat.push_agent_delta("hello");
        chat.push_agent_delta(" world");
        chat.finish_streaming();
        assert_eq!(chat.cells.len(), 1);
        assert!(!chat.has_active_assistant());
    }
}
