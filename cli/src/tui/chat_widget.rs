use ratatui::Frame;
use ratatui::layout::Rect;

use crate::tui::history_cell::HistoryCell;
use crate::tui::theme::Theme;

/// A scrollable, cell-based chat widget that owns a list of [`HistoryCell`]s.
///
/// Each cell knows its own height via [`HistoryCell::needed_lines`] and renders
/// itself into a sub-area of the widget. The widget handles scrolling by
/// computing which cells are visible given the current `scroll_offset` and the
/// available screen height.
pub struct ChatWidget {
    /// Ordered conversation cells.
    cells: Vec<Box<dyn HistoryCell>>,
    /// Index of the last streaming cell (for push_text).
    active_idx: Option<usize>,
    /// Scroll offset from bottom (0 = bottom).
    pub scroll_offset: usize,
}

impl ChatWidget {
    pub fn new() -> Self {
        Self { cells: Vec::new(), active_idx: None, scroll_offset: 0 }
    }

    /// Append a new cell to the conversation.
    pub fn push_cell(&mut self, cell: Box<dyn HistoryCell>) {
        self.active_idx = if cell.is_streaming() { Some(self.cells.len()) } else { None };
        self.cells.push(cell);
        self.scroll_to_bottom();
    }

    /// Find and update an existing cell by predicate, or push a new one.
    /// Returns the index of the cell.
    pub fn upsert_cell<F>(&mut self, new: Box<dyn HistoryCell>, matcher: F)
    where
        F: Fn(&dyn HistoryCell) -> bool,
    {
        if let Some(pos) = self.cells.iter().position(|c| matcher(c.as_ref())) {
            self.cells[pos] = new;
        } else {
            self.cells.push(new);
        }
        self.scroll_to_bottom();
    }

    /// Get mutable reference to the last streaming cell.
    pub fn active_mut(&mut self) -> Option<&mut (dyn HistoryCell + 'static)> {
        let idx = self.active_idx?;
        self.cells.get_mut(idx).map(Box::as_mut)
    }

    /// Append text to the streaming cell.
    pub fn push_text(&mut self, text: &str) {
        if let Some(idx) = self.active_idx
            && let Some(cell) = self.cells.get_mut(idx)
        {
            cell.push_text(text);
        }
    }

    /// Remove a ToolCallCell by its tool_call_id.
    pub fn remove_tool_call(&mut self, id: &str) {
        self.cells.retain(|c| c.tool_call_id() != Some(id));
    }

    /// Find a cell by tool_call_id.
    pub fn find_tool_call(&self, id: &str) -> Option<&(dyn HistoryCell + 'static)> {
        self.cells.iter().find(|c| c.tool_call_id() == Some(id)).map(Box::as_ref)
    }

    /// Find a cell by tool_call_id (mutable).
    pub fn find_tool_call_mut(&mut self, id: &str) -> Option<&mut (dyn HistoryCell + 'static)> {
        self.cells.iter_mut().find(|c| c.tool_call_id() == Some(id)).map(Box::as_mut)
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn clear(&mut self) {
        self.cells.clear();
        self.active_idx = None;
        self.scroll_offset = 0;
    }

    /// Total height (in terminal lines) of all cells at the given width.
    pub fn total_height(&self, width: usize) -> u16 {
        self.cells.iter().map(|c| c.needed_lines(width)).sum()
    }

    /// Render the conversation into the given area.
    ///
    /// Only cells that overlap the visible window (computed from
    /// `scroll_offset` and `area.height`) are rendered. Each visible
    /// cell draws itself into a sub-area at its computed y offset.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.cells.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }

        let width = area.width as usize;
        let total = self.total_height(width);
        let area_height = area.height;

        // Visible range: which total-line indices are visible.
        let visible_end = total.saturating_sub(self.scroll_offset as u16);
        let visible_start = visible_end.saturating_sub(area_height);

        let mut acc = 0u16; // accumulated line count before current cell

        for cell in &self.cells {
            let cell_lines = cell.needed_lines(width);

            if cell_lines == 0 {
                continue;
            }

            let cell_end = acc + cell_lines;

            // Skip cells entirely before the visible window.
            if cell_end <= visible_start {
                acc += cell_lines;
                continue;
            }

            // Stop once we are past the visible window.
            if acc >= visible_end {
                break;
            }

            // This cell is at least partially visible.
            let visible_part_start = acc.max(visible_start);
            let visible_part_end = cell_end.min(visible_end);
            let visible_height = visible_part_end - visible_part_start;

            // Y offset within the display area.
            let display_y = area.y + (visible_part_start - visible_start);

            let cell_area =
                Rect { x: area.x, y: display_y, width: area.width, height: visible_height };

            cell.render(frame, cell_area, theme);
            acc += cell_lines;
        }
    }
}

impl Default for ChatWidget {
    fn default() -> Self {
        Self::new()
    }
}
