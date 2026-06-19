use ratatui::Frame;
use ratatui::layout::Rect;

use crate::tui::history_cell::{AgentCell, HistoryCell, ThinkingCell, ToolCallCell};
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
    /// Index of the active assistant streaming cell.
    assistant_active_idx: Option<usize>,
    /// Index of the active thinking streaming cell.
    thinking_active_idx: Option<usize>,
    /// Currently selected cell for keyboard actions.
    selected_idx: Option<usize>,
    /// Scroll offset from bottom (0 = bottom).
    pub scroll_offset: usize,
}

impl ChatWidget {
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            active_idx: None,
            assistant_active_idx: None,
            thinking_active_idx: None,
            selected_idx: None,
            scroll_offset: 0,
        }
    }

    /// Append a new cell to the conversation.
    pub fn push_cell(&mut self, cell: Box<dyn HistoryCell>) {
        self.active_idx = if cell.is_streaming() { Some(self.cells.len()) } else { None };
        self.cells.push(cell);
        self.scroll_to_bottom();
    }

    pub fn push_agent_delta(&mut self, text: &str) {
        if let Some(idx) = self.assistant_active_idx
            && let Some(cell) = self.cells.get_mut(idx)
            && cell.as_any().is::<AgentCell>()
        {
            cell.push_text(text);
            self.scroll_to_bottom();
            return;
        }
        let idx = self.cells.len();
        self.cells.push(Box::new(AgentCell { buffer: text.to_string(), is_streaming: true }));
        self.assistant_active_idx = Some(idx);
        self.active_idx = Some(idx);
        self.scroll_to_bottom();
    }

    pub fn push_thinking_delta(&mut self, text: &str) {
        if let Some(idx) = self.thinking_active_idx
            && let Some(cell) = self.cells.get_mut(idx)
            && cell.as_any().is::<ThinkingCell>()
        {
            cell.push_text(text);
            self.scroll_to_bottom();
            return;
        }
        let idx = self.cells.len();
        self.cells.push(Box::new(ThinkingCell { buffer: text.to_string(), is_streaming: true }));
        self.thinking_active_idx = Some(idx);
        self.active_idx = Some(idx);
        self.scroll_to_bottom();
    }

    pub fn finish_streaming_cells(&mut self) {
        for idx in [self.assistant_active_idx.take(), self.thinking_active_idx.take()]
            .into_iter()
            .flatten()
        {
            if let Some(cell) = self.cells.get_mut(idx) {
                cell.finish_streaming();
            }
        }
        self.active_idx = None;
    }

    pub fn has_active_assistant(&self) -> bool {
        self.assistant_active_idx.is_some()
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
        self.repair_indices();
    }

    /// Find a cell by tool_call_id.
    pub fn find_tool_call(&self, id: &str) -> Option<&(dyn HistoryCell + 'static)> {
        self.cells.iter().find(|c| c.tool_call_id() == Some(id)).map(Box::as_ref)
    }

    /// Find a cell by tool_call_id (mutable).
    pub fn find_tool_call_mut(&mut self, id: &str) -> Option<&mut (dyn HistoryCell + 'static)> {
        self.cells.iter_mut().find(|c| c.tool_call_id() == Some(id)).map(Box::as_mut)
    }

    pub fn select_next_tool(&mut self) {
        self.move_tool_selection(1);
    }

    pub fn select_prev_tool(&mut self) {
        self.move_tool_selection(-1);
    }

    pub fn toggle_selected_tool(&mut self) -> bool {
        let Some(idx) = self.selected_idx else { return false };
        let Some(cell) = self.cells.get_mut(idx) else { return false };
        if let Some(tool) = cell.as_any_mut().downcast_mut::<ToolCallCell>() {
            tool.toggle_expand();
            self.scroll_to_bottom();
            return true;
        }
        false
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
        self.assistant_active_idx = None;
        self.thinking_active_idx = None;
        self.selected_idx = None;
        self.scroll_offset = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    fn move_tool_selection(&mut self, delta: isize) {
        let selectable: Vec<usize> = self
            .cells
            .iter()
            .enumerate()
            .filter_map(|(idx, cell)| cell.is_selectable().then_some(idx))
            .collect();
        if selectable.is_empty() {
            return;
        }
        let current_pos = self
            .selected_idx
            .and_then(|idx| selectable.iter().position(|candidate| *candidate == idx));
        let next_pos = match current_pos {
            Some(pos) => {
                let len = selectable.len() as isize;
                (pos as isize + delta).rem_euclid(len) as usize
            }
            None => {
                if delta < 0 {
                    selectable.len() - 1
                } else {
                    0
                }
            }
        };
        self.set_selected_idx(Some(selectable[next_pos]));
    }

    fn set_selected_idx(&mut self, idx: Option<usize>) {
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

    fn repair_indices(&mut self) {
        self.active_idx = None;
        self.assistant_active_idx = None;
        self.thinking_active_idx = None;
        self.selected_idx = None;
        for cell in &mut self.cells {
            cell.set_selected(false);
        }
    }

    /// Total height (in terminal lines) of all cells at the given width.
    pub fn total_height(&self, width: usize) -> u16 {
        self.cells.iter().map(|c| c.needed_lines(width)).sum()
    }

    fn clamp_scroll_offset(&mut self, width: usize, viewport_height: u16) {
        let max_offset = self.total_height(width).saturating_sub(viewport_height) as usize;
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    /// Render the conversation into the given area.
    ///
    /// Only cells that overlap the visible window (computed from
    /// `scroll_offset` and `area.height`) are rendered. Each visible
    /// cell draws itself into a sub-area at its computed y offset.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.cells.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }

        let width = area.width as usize;
        let total = self.total_height(width);
        let area_height = area.height;
        self.clamp_scroll_offset(width, area_height);

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

            let top_skip = visible_part_start - acc;
            let display_y = area.y + (visible_part_start - visible_start);

            let cell_area =
                Rect { x: area.x, y: display_y, width: area.width, height: visible_height };

            cell.render_scrolled(frame, cell_area, theme, top_skip);
            acc += cell_lines;
        }
    }
}

impl Default for ChatWidget {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn assistant_and_thinking_deltas_use_separate_cells() {
        let mut chat = ChatWidget::new();

        chat.push_thinking_delta("thinking");
        chat.push_agent_delta("answer");

        assert_eq!(chat.cells.len(), 2);
        assert!(chat.cells[0].as_any().is::<ThinkingCell>());
        assert!(chat.cells[1].as_any().is::<AgentCell>());

        let thinking = chat.cells[0].as_any().downcast_ref::<ThinkingCell>().unwrap();
        let assistant = chat.cells[1].as_any().downcast_ref::<AgentCell>().unwrap();
        assert_eq!(thinking.buffer, "thinking");
        assert_eq!(assistant.buffer, "answer");
    }

    #[test]
    fn finish_streaming_cells_marks_active_cells_complete() {
        let mut chat = ChatWidget::new();

        chat.push_thinking_delta("thinking");
        chat.push_agent_delta("answer");
        chat.finish_streaming_cells();

        let thinking = chat.cells[0].as_any().downcast_ref::<ThinkingCell>().unwrap();
        let assistant = chat.cells[1].as_any().downcast_ref::<AgentCell>().unwrap();
        assert!(!thinking.is_streaming);
        assert!(!assistant.is_streaming);
        assert!(!chat.has_active_assistant());
    }

    #[test]
    fn selected_tool_can_toggle_expansion_without_losing_progress() {
        let mut chat = ChatWidget::new();
        let mut tool = ToolCallCell::new("call-1".into(), "Bash".into(), "echo hi".into());
        tool.add_progress("line 1".into());
        tool.add_result_content(&serde_json::json!({"stdout": "ok\nnext\n", "stderr": ""}), false);
        chat.push_cell(Box::new(tool));

        chat.select_next_tool();
        assert!(chat.toggle_selected_tool());

        let tool = chat.cells[0].as_any().downcast_ref::<ToolCallCell>().unwrap();
        assert!(tool.expanded);
        assert_eq!(tool.progress_messages, vec!["line 1"]);
        assert_eq!(tool.result_lines, vec!["ok", "next"]);
    }

    #[test]
    fn overscrolling_past_top_still_renders_history() {
        let mut chat = ChatWidget::new();
        chat.push_cell(Box::new(AgentCell {
            buffer: "one\ntwo\nthree".to_string(),
            is_streaming: false,
        }));

        chat.scroll_up(100);

        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::default();
        terminal.draw(|frame| chat.render(frame, frame.area(), &theme)).unwrap();

        assert_eq!(chat.scroll_offset, 0);

        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("one"), "{rendered:?}");
        assert!(rendered.contains("three"), "{rendered:?}");
    }

    #[test]
    fn scrolling_into_middle_of_cell_keeps_correct_visible_lines() {
        let mut chat = ChatWidget::new();
        chat.push_cell(Box::new(AgentCell {
            buffer: "one\ntwo\nthree\nfour\nfive".to_string(),
            is_streaming: false,
        }));

        chat.scroll_up(2);

        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::default();
        terminal.draw(|frame| chat.render(frame, frame.area(), &theme)).unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("one"), "{rendered:?}");
        assert!(rendered.contains("two"), "{rendered:?}");
        assert!(rendered.contains("three"), "{rendered:?}");
        assert!(!rendered.contains("four"), "{rendered:?}");
        assert!(!rendered.contains("five"), "{rendered:?}");
    }
}
