use crossterm::event::{KeyCode, KeyEvent};
use std::any::Any;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::tui::overlay::{Overlay, OverlayAction};
use crate::tui::theme::Theme;

/// Maximum visible items in the popup before scrolling kicks in.
const MAX_VISIBLE: usize = 12;

/// A generic list selection popup.
///
/// Shows a scrollable list of items. User navigates with Up/Down.
/// Enter selects, Esc cancels.
pub struct SelectionPopup {
    title: String,
    items: Vec<String>,
    selected: usize,
    scroll_offset: usize,
    result: Option<Option<usize>>, // None = cancelled, Some(Some(idx)) = selected
}

impl SelectionPopup {
    pub fn new(title: impl Into<String>, items: Vec<impl Into<String>>) -> Self {
        let items: Vec<String> = items.into_iter().map(Into::into).collect();
        Self { title: title.into(), items, selected: 0, scroll_offset: 0, result: None }
    }

    /// The result after this popup has been popped.
    /// Returns `None` if cancelled, `Some(idx)` if an item was selected.
    pub fn selected_index(&self) -> Option<usize> {
        self.result?
    }

    /// The selected item text (useful for display).
    pub fn selected_item(&self) -> Option<&str> {
        self.selected_index().and_then(|i| self.items.get(i).map(|s| s.as_str()))
    }

    fn visible_items(&self, max_height: usize) -> &[String] {
        let start = self.scroll_offset;
        let end = (start + max_height).min(self.items.len());
        &self.items[start..end]
    }
}

impl Overlay for SelectionPopup {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let popup_w = area.width.saturating_sub(10).clamp(30, 50);
        let max_visible = (area.height.saturating_sub(6) as usize).min(MAX_VISIBLE);
        let popup_h = (self.items.len().min(max_visible) as u16).saturating_add(3).max(5);

        let popup_area = Rect {
            x: area.x + (area.width.saturating_sub(popup_w)) / 2,
            y: area.y + (area.height.saturating_sub(popup_h)) / 2,
            width: popup_w,
            height: popup_h,
        };

        let block = Block::default()
            .title(self.title.as_ref())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_active));

        let visible = self.visible_items(max_visible);
        let mut lines: Vec<Line> = Vec::with_capacity(visible.len());
        for (i, item) in visible.iter().enumerate() {
            let abs_idx = self.scroll_offset + i;
            let is_selected = abs_idx == self.selected;
            let style = if is_selected {
                Style::default().fg(theme.border_active).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(theme.assistant_fg)
            };
            let marker = if is_selected { "▸ " } else { "  " };
            lines.push(Line::from(Span::styled(format!("{}{}", marker, item), style)));
        }

        let paragraph = Paragraph::new(Text::from(lines)).block(block);
        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        // Guard: empty items — nothing to select.
        if self.items.is_empty() {
            return OverlayAction::None;
        }
        match key.code {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                    if self.selected < self.scroll_offset {
                        self.scroll_offset = self.selected;
                    }
                }
                OverlayAction::None
            }
            KeyCode::Down => {
                if self.selected + 1 < self.items.len() {
                    self.selected += 1;
                    let max_visible = MAX_VISIBLE;
                    if self.selected >= self.scroll_offset + max_visible {
                        self.scroll_offset =
                            self.selected.saturating_sub(max_visible.saturating_sub(1));
                    }
                }
                OverlayAction::None
            }
            KeyCode::Enter => {
                self.result = Some(Some(self.selected));
                OverlayAction::Pop
            }
            KeyCode::Esc => {
                self.result = Some(None);
                OverlayAction::Pop
            }
            _ => OverlayAction::None,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
