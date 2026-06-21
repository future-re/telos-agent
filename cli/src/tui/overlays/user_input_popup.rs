use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::any::Any;
use std::collections::HashMap;

use crate::tui::overlay::{Overlay, OverlayAction};
use crate::tui::theme::Theme;

/// A multi-question input form popup.
///
/// Renders each question with an editable text field.
/// Tab/Shift+Tab navigates between fields. Enter submits all.
/// Esc cancels.
pub struct UserInputPopup {
    title: String,
    questions: Vec<Question>,
    active_field: usize,
    result: Option<Option<HashMap<String, String>>>,
    context: Option<String>,
    error: Option<String>,
}

pub struct Question {
    pub key: String,
    pub label: String,
    pub value: String,
    pub placeholder: String,
}

impl UserInputPopup {
    pub fn new(title: impl Into<String>, questions: Vec<Question>) -> Self {
        Self {
            title: title.into(),
            questions,
            active_field: 0,
            result: None,
            context: None,
            error: None,
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    pub fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    /// The collected answers after submission, or None if cancelled.
    pub fn answers(&self) -> Option<&HashMap<String, String>> {
        self.result.as_ref()?.as_ref()
    }
}

impl Overlay for UserInputPopup {
    fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let popup_w = area.width.saturating_sub(10).clamp(40, 60);
        let field_count = self.questions.len();
        let popup_h = (field_count as u16 * 3 + 4).max(8).min(area.height.saturating_sub(4));

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

        let mut lines: Vec<Line> = Vec::new();
        for (i, q) in self.questions.iter().enumerate() {
            let is_active = i == self.active_field;
            let label_style = if is_active {
                Style::default().fg(theme.border_active).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.assistant_fg)
            };
            lines.push(Line::from(Span::styled(format!("  {} ", q.label), label_style)));

            let display = if q.value.is_empty() {
                format!("  [{}]", q.placeholder)
            } else {
                format!("  {}", q.value)
            };
            let input_style = if is_active {
                Style::default().fg(theme.user_fg).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(theme.user_fg)
            };
            lines.push(Line::from(Span::styled(display, input_style)));
            lines.push(Line::from(""));
        }
        if let Some(error) = &self.error {
            lines.push(Line::from(Span::styled(
                format!("  {error}"),
                Style::default().fg(theme.tool_error_fg),
            )));
        }

        lines.push(Line::from(Span::styled(
            "  Tab·next  Enter·submit  Esc·cancel",
            Style::default().fg(theme.thinking_fg).add_modifier(Modifier::DIM),
        )));

        let paragraph = Paragraph::new(Text::from(lines)).block(block);
        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        // Guard: empty questions = nothing to interact with; avoid div-by-zero.
        if self.questions.is_empty() {
            return OverlayAction::None;
        }
        match key.code {
            KeyCode::Tab => {
                self.active_field = (self.active_field + 1) % self.questions.len();
                OverlayAction::None
            }
            KeyCode::BackTab => {
                self.active_field = if self.active_field == 0 {
                    self.questions.len() - 1
                } else {
                    self.active_field - 1
                };
                OverlayAction::None
            }
            KeyCode::Char(c) => {
                if let Some(q) = self.questions.get_mut(self.active_field) {
                    q.value.push(c);
                }
                self.error = None;
                OverlayAction::None
            }
            KeyCode::Backspace => {
                if let Some(q) = self.questions.get_mut(self.active_field) {
                    q.value.pop();
                }
                self.error = None;
                OverlayAction::None
            }
            KeyCode::Enter => {
                let mut map = HashMap::new();
                for q in &self.questions {
                    let val =
                        if q.value.is_empty() { q.placeholder.clone() } else { q.value.clone() };
                    map.insert(q.key.clone(), val);
                }
                self.result = Some(Some(map));
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
