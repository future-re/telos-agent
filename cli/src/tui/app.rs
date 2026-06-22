//! TUI application state and event loop.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::tui::approval::ApprovalOverlay;
use crate::tui::chat::ChatWidget;
use crate::tui::composer::Composer;
use crate::tui::history_cell::UserCell;
use crate::tui::render::Renderable;
use crate::tui::status::StatusBar;

use crate::tui::keymap::is_ctrl_char;

pub enum AppEvent {
    Quit,
    Submit(String),
    Tick,
    Key(crossterm::event::KeyEvent),
    Paste(String),
    Mouse(crossterm::event::MouseEvent),
    Resize,
}

pub struct App {
    pub chat: ChatWidget,
    pub composer: Composer,
    pub status_bar: StatusBar,
    pub approval: ApprovalOverlay,
    pub should_quit: bool,
    pub turn_active: bool,
    pub auto_mode: Arc<AtomicBool>,
    pub spinner_frame: usize,
    pub status_text: String,
    base_status: String,
    _turn_started: Option<Instant>,
    _turn_tool_calls: u64,
    _turn_tool_failures: u64,
}

impl App {
    pub fn new(status_text: String, auto_mode: Arc<AtomicBool>) -> Self {
        let base = status_text.trim_end_matches(" · auto").to_string();
        Self {
            chat: ChatWidget::new(),
            composer: Composer::new(),
            status_bar: StatusBar::new(status_text.clone()),
            approval: ApprovalOverlay::new(),
            should_quit: false,
            turn_active: false,
            auto_mode,
            spinner_frame: 0,
            status_text,
            base_status: base,
            _turn_started: None,
            _turn_tool_calls: 0,
            _turn_tool_failures: 0,
        }
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(key) => {
                // Global: Ctrl+D on empty input quits
                if is_ctrl_char(key, 'd') && self.composer.is_empty() {
                    self.should_quit = true;
                    return;
                }
                // Global: Ctrl+C cancels
                if is_ctrl_char(key, 'c') {
                    self.turn_active = false;
                    self.composer.clear();
                    self.status_text = self.base_status.clone();
                    return;
                }
                // Global: Ctrl+L clears
                if is_ctrl_char(key, 'l') {
                    self.chat.clear();
                    self.chat.scroll_to_bottom();
                    return;
                }
                // Mode-specific
                if self.composer.active {
                    self.composer.handle_key(key);
                }
                // Arrow keys for scrolling
                match (key.code, key.modifiers) {
                    (KeyCode::Up, KeyModifiers::NONE) => self.chat.scroll_up(1),
                    (KeyCode::Down, KeyModifiers::NONE) => self.chat.scroll_down(1),
                    (KeyCode::PageUp, _) => self.chat.scroll_up(10),
                    (KeyCode::PageDown, _) => self.chat.scroll_down(10),
                    (KeyCode::Tab, _) => self.chat.select_next_tool(),
                    (KeyCode::Char('t'), modifiers)
                        if modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        self.chat.toggle_selected();
                    }
                    _ => {}
                }
            }
            AppEvent::Submit(text) => {
                if !text.is_empty() {
                    self.chat.push_cell(Box::new(UserCell { content: text.clone() }));
                    self.composer.record_history(text);
                    self.composer.clear();
                    self.turn_active = true;
                }
            }
            AppEvent::Tick => {
                self.spinner_frame = self.spinner_frame.wrapping_add(1);
            }
            AppEvent::Paste(text) => {
                self.composer.insert_text(&text);
            }
            _ => {}
        }
    }

    /// Build the full screen layout and render.
    pub fn draw(&self, area: Rect, buf: &mut Buffer) {
        // Simple layout: chat (flex) | composer (fixed) | status (fixed)
        let composer_h = self.composer.desired_height(area.width);
        let status_h = 1u16;
        let chat_h = area.height.saturating_sub(composer_h).saturating_sub(status_h);

        let chat_area = Rect { x: area.x, y: area.y, width: area.width, height: chat_h };
        let composer_area =
            Rect { x: area.x, y: area.y + chat_h, width: area.width, height: composer_h };
        let status_area = Rect {
            x: area.x,
            y: area.y + chat_h + composer_h,
            width: area.width,
            height: status_h,
        };

        self.chat.render(chat_area, buf);
        self.composer.render(composer_area, buf);
        self.status_bar.render(status_area, buf);

        // Render approval overlay on top of everything.
        if self.approval.is_visible() {
            self.approval.render(area, buf);
        }
    }
}
