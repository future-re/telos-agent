//! Scrollable chat widget backed by a flat list of [`ChatEntry`] values.
//!
//! Rendering collects lines from every entry via [`ChatEntry::to_lines`],
//! concatenates them into one [`Paragraph`], and scrolls with
//! [`Paragraph::scroll`]. Because measurement and rendering share the
//! same code path, layout is always consistent.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::chat_entry::ChatEntry;
use crate::tui::theme::Theme;

pub struct ChatWidget {
    /// All conversation entries in chronological order.
    entries: Vec<ChatEntry>,
    /// Index of the last streaming entry (for `push_text`).
    active_idx: Option<usize>,
    /// Index of the active assistant streaming entry.
    assistant_active_idx: Option<usize>,
    /// Index of the active thinking streaming entry.
    thinking_active_idx: Option<usize>,
    /// Currently selected entry for keyboard actions.
    selected_idx: Option<usize>,
    /// Scroll offset from bottom (0 = bottom).
    pub scroll_offset: usize,
    /// Whether the user has manually scrolled up (prevents auto-scroll-to-bottom).
    user_scrolled_up: bool,
}

impl ChatWidget {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            active_idx: None,
            assistant_active_idx: None,
            thinking_active_idx: None,
            selected_idx: None,
            scroll_offset: 0,
            user_scrolled_up: false,
        }
    }

    // ── Mutation ──────────────────────────────────────────────────────────

    /// Append a new entry.
    pub fn push_entry(&mut self, entry: ChatEntry) {
        if entry.is_streaming() {
            self.active_idx = Some(self.entries.len());
        }
        self.entries.push(entry);
        self.maybe_scroll_to_bottom();
    }

    /// Stream a delta into the active assistant cell, creating one if needed.
    pub fn push_agent_delta(&mut self, text: &str) {
        if let Some(idx) = self.assistant_active_idx
            && let Some(ChatEntry::Agent { is_streaming: true, .. }) = self.entries.get_mut(idx)
        {
            self.entries[idx].push_text(text);
            self.maybe_scroll_to_bottom();
            return;
        }
        // Create a new agent streaming cell.
        let entry = ChatEntry::agent(text.to_string(), true);
        let idx = self.entries.len();
        self.entries.push(entry);
        self.assistant_active_idx = Some(idx);
        self.active_idx = Some(idx);
        self.maybe_scroll_to_bottom();
    }

    /// Stream a delta into the active thinking cell, creating one if needed.
    pub fn push_thinking_delta(&mut self, text: &str) {
        if let Some(idx) = self.thinking_active_idx
            && let Some(ChatEntry::Thinking { is_streaming: true, .. }) = self.entries.get_mut(idx)
        {
            self.entries[idx].push_text(text);
            self.maybe_scroll_to_bottom();
            return;
        }
        let entry = ChatEntry::thinking(text.to_string(), true);
        let idx = self.entries.len();
        self.entries.push(entry);
        self.thinking_active_idx = Some(idx);
        self.active_idx = Some(idx);
        self.maybe_scroll_to_bottom();
    }

    /// Mark all streaming cells as finished.
    pub fn finish_streaming_cells(&mut self) {
        for idx in [self.assistant_active_idx.take(), self.thinking_active_idx.take()]
            .into_iter()
            .flatten()
        {
            if let Some(entry) = self.entries.get_mut(idx) {
                entry.finish_streaming();
            }
        }
        self.active_idx = None;
    }

    /// Whether the assistant cell is still streaming.
    pub fn has_active_assistant(&self) -> bool {
        self.assistant_active_idx.is_some()
    }

    pub fn active_mut(&mut self) -> Option<&mut ChatEntry> {
        let idx = self.active_idx?;
        self.entries.get_mut(idx)
    }

    /// Find an entry by tool_call_id.
    pub fn find_tool_call(&self, id: &str) -> Option<&ChatEntry> {
        self.entries.iter().find(|e| e.tool_call_id() == Some(id))
    }

    /// Find an entry by tool_call_id (mutable).
    pub fn find_tool_call_mut(&mut self, id: &str) -> Option<&mut ChatEntry> {
        self.entries.iter_mut().find(|e| e.tool_call_id() == Some(id))
    }

    /// Remove a tool call by its id.
    pub fn remove_tool_call(&mut self, id: &str) {
        let old_len = self.entries.len();
        self.entries.retain(|e| e.tool_call_id() != Some(id));
        if self.entries.len() != old_len {
            self.repair_indices();
        }
    }

    // ── Selection ─────────────────────────────────────────────────────────

    pub fn select_next_tool(&mut self) {
        self.move_tool_selection(1);
    }

    pub fn select_prev_tool(&mut self) {
        self.move_tool_selection(-1);
    }

    pub fn toggle_selected_tool(&mut self) -> bool {
        let Some(idx) = self.selected_idx else { return false };
        if let Some(ChatEntry::ToolCall { expanded, .. }) = self.entries.get_mut(idx) {
            *expanded = !*expanded;
            self.maybe_scroll_to_bottom();
            return true;
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
        self.entries.clear();
        self.active_idx = None;
        self.assistant_active_idx = None;
        self.thinking_active_idx = None;
        self.selected_idx = None;
        self.scroll_offset = 0;
        self.user_scrolled_up = false;
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    // ── Internals ─────────────────────────────────────────────────────────

    fn move_tool_selection(&mut self, delta: isize) {
        let selectable: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| entry.is_selectable().then_some(idx))
            .collect();
        if selectable.is_empty() {
            return;
        }
        let current_pos =
            self.selected_idx.and_then(|idx| selectable.iter().position(|c| *c == idx));
        let next_pos = match current_pos {
            Some(pos) => (pos as isize + delta).rem_euclid(selectable.len() as isize) as usize,
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
            && let Some(entry) = self.entries.get_mut(old)
        {
            entry.set_selected(false);
        }
        self.selected_idx = idx;
        if let Some(new) = self.selected_idx
            && let Some(entry) = self.entries.get_mut(new)
        {
            entry.set_selected(true);
        }
    }

    fn repair_indices(&mut self) {
        self.active_idx = None;
        self.assistant_active_idx = None;
        self.thinking_active_idx = None;
        self.selected_idx = None;
        for entry in &mut self.entries {
            entry.set_selected(false);
        }
    }

    // ── Rendering ─────────────────────────────────────────────────────────

    /// Render all entries into `area`, scrolled to `scroll_offset` lines
    /// from the bottom.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.entries.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }

        let width = area.width as usize;

        // Collect all lines from all entries.
        let mut all_lines: Vec<ratatui::text::Line<'static>> = Vec::new();
        for entry in &self.entries {
            all_lines.extend(entry.to_lines(width, theme));
        }

        let total = all_lines.len();
        let visible = area.height as usize;

        // Bottom-anchor: when content is shorter than the viewport,
        // pad with blank lines at the top so messages start at the bottom.
        if total < visible {
            let pad = visible - total;
            let mut padded = vec![ratatui::text::Line::from(""); pad];
            padded.append(&mut all_lines);
            all_lines = padded;
        }

        let total = all_lines.len();
        // scroll_offset is "lines from bottom" (0 = bottom).
        // Paragraph::scroll expects "lines to skip from top", so convert.
        let max_scroll = total.saturating_sub(visible);
        self.scroll_offset = self.scroll_offset.min(max_scroll);
        let paragraph_scroll = max_scroll.saturating_sub(self.scroll_offset) as u16;

        frame.render_widget(
            Paragraph::new(Text::from(all_lines))
                .scroll((paragraph_scroll, 0))
                .wrap(Wrap { trim: false }),
            area,
        );
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

    #[test]
    fn assistant_and_thinking_deltas_use_separate_entries() {
        let mut chat = ChatWidget::new();

        chat.push_thinking_delta("thinking");
        chat.push_agent_delta("answer");

        assert_eq!(chat.entries.len(), 2);
        assert!(matches!(chat.entries[0], ChatEntry::Thinking { .. }));
        assert!(matches!(chat.entries[1], ChatEntry::Agent { .. }));
    }

    #[test]
    fn finish_streaming_cells_marks_active_entries_complete() {
        let mut chat = ChatWidget::new();

        chat.push_thinking_delta("thinking");
        chat.push_agent_delta("answer");
        chat.finish_streaming_cells();

        assert!(!chat.has_active_assistant());
        assert!(!chat.entries[0].is_streaming());
        assert!(!chat.entries[1].is_streaming());
    }

    #[test]
    fn toggle_selected_tool_expands_it() {
        let mut chat = ChatWidget::new();
        let tool = ChatEntry::tool_call("call-1".into(), "Bash".into(), "echo hi".into());
        chat.push_entry(tool);
        chat.select_next_tool();

        assert!(chat.toggle_selected_tool());
    }

    #[test]
    fn scroll_to_bottom_resets_user_scrolled_up() {
        let mut chat = ChatWidget::new();
        chat.push_entry(ChatEntry::user("hello".into()));
        chat.scroll_up(3);
        assert!(chat.scroll_offset > 0);

        chat.scroll_to_bottom();
        assert_eq!(chat.scroll_offset, 0);
    }

    #[test]
    fn user_scrolled_up_prevents_auto_scroll() {
        let mut chat = ChatWidget::new();
        chat.push_entry(ChatEntry::user("first".into()));
        chat.scroll_up(5);
        assert!(chat.scroll_offset > 0);

        // Pushing a new entry should NOT reset scroll when user scrolled up.
        chat.push_entry(ChatEntry::agent("second".into(), false));
        assert!(chat.scroll_offset > 0);
    }
}
