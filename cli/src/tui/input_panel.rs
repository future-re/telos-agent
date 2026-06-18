use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;

#[derive(Debug, Default)]
pub struct InputPanel;
impl InputPanel {
    pub fn new() -> Self {
        Self
    }
    pub fn is_empty(&self) -> bool {
        true
    }
    pub fn handle_key(&mut self, _key: KeyEvent) -> Option<String> {
        None
    }
    pub fn render(&self, _frame: &mut Frame, _area: Rect, _active: bool) {}
}
