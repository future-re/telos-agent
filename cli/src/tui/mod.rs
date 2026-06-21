pub mod app;
#[path = "overlays/approval.rs"]
pub mod approval;
#[path = "widgets/approval_inline.rs"]
pub mod approval_inline;
pub mod chat_entry;
#[path = "widgets/chat_widget.rs"]
pub mod chat_widget;
#[path = "overlays/command_popup.rs"]
pub mod command_popup;
pub mod event;
#[path = "widgets/input_panel.rs"]
pub mod input_panel;
pub mod keymap;
pub mod markdown;
#[path = "overlays/overlay.rs"]
pub mod overlay;
#[path = "overlays/selection_popup.rs"]
pub mod selection_popup;
#[path = "widgets/status_bar.rs"]
pub mod status_bar;
pub mod theme;
#[path = "widgets/tool_rendering.rs"]
mod tool_rendering;
#[path = "overlays/user_input_popup.rs"]
pub mod user_input_popup;

use crate::config::BillingSection;
use crate::tui::app::{App, ModelSwitchConfig, TuiLayoutSettings};
use crate::tui::event::Event;
use anyhow::Result;
use crossterm::event::{
    Event as CEvent, EventStream, KeyEventKind, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::stdout;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use telos_agent::{AgentConfig, MemoryStore, ModelProvider, ToolRegistry};

/// Ensures the terminal leaves raw mode and the alternate screen on panic or
/// early return.
struct TuiGuard {
    keyboard_enhancement_enabled: bool,
}

impl Drop for TuiGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        if self.keyboard_enhancement_enabled {
            let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableBracketedPaste,
            crossterm::terminal::LeaveAlternateScreen
        );
    }
}

/// Launch the ratatui full-screen TUI.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    config: AgentConfig,
    provider: Arc<dyn ModelProvider>,
    tools: ToolRegistry,
    status_text: String,
    project_root: Option<&std::path::Path>,
    project_root_or_cwd: &std::path::Path,
    auto_mode: bool,
    memory_store: Arc<Mutex<MemoryStore>>,
    model_switch: ModelSwitchConfig,
    layout_settings: TuiLayoutSettings,
    billing: Option<BillingSection>,
) -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    // Guard ensures raw mode is always disabled on drop, even if
    // EnterAlternateScreen or keyboard enhancement fails.
    let mut guard = TuiGuard { keyboard_enhancement_enabled: false };

    let mut stdout = stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    guard.keyboard_enhancement_enabled =
        matches!(crossterm::terminal::supports_keyboard_enhancement(), Ok(true))
            && crossterm::execute!(
                stdout,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )
            .is_ok();
    let _ = crossterm::execute!(stdout, crossterm::event::EnableBracketedPaste);

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new_with_layout_settings(
        config,
        provider,
        tools,
        status_text,
        project_root,
        project_root_or_cwd,
        auto_mode,
        memory_store,
        model_switch,
        layout_settings,
        billing,
    )?;
    let tick_rate = Duration::from_millis(100);
    let mut reader = EventStream::new();

    loop {
        terminal.draw(|frame| app.draw(frame))?;

        let event = tokio::select! {
            maybe_event = reader.next() => {
                match maybe_event {
                    Some(Ok(event)) => match crossterm_event_to_app_event(event) {
                        Some(event) => event,
                        None => continue,
                    },
                    _ => continue,
                }
            }
            _ = tokio::time::sleep(tick_rate) => Event::Tick,
        };

        if let Err(e) = app.handle_event(event).await {
            eprintln!("TUI event handling error: {e}");
            break;
        }
        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn crossterm_event_to_app_event(event: CEvent) -> Option<Event> {
    match event {
        CEvent::Key(key) if key.kind == KeyEventKind::Release => None,
        CEvent::Key(key) => Some(Event::Key(key)),
        CEvent::Mouse(mouse) => Some(Event::Mouse(mouse)),
        CEvent::Paste(text) => Some(Event::Paste(text)),
        CEvent::Resize(cols, rows) => Some(Event::Resize { cols, rows }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn crossterm_event_mapping_ignores_key_release_events() {
        let event = CEvent::Key(KeyEvent::new_with_kind(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
            KeyEventKind::Release,
        ));

        assert!(crossterm_event_to_app_event(event).is_none());
    }

    #[test]
    fn crossterm_event_mapping_keeps_key_press_events() {
        let event = CEvent::Key(KeyEvent::new_with_kind(
            KeyCode::Up,
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        ));

        assert!(matches!(crossterm_event_to_app_event(event), Some(Event::Key(_))));
    }
}
