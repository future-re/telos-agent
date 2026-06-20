pub mod app;
#[path = "overlays/approval.rs"]
pub mod approval;
#[path = "widgets/approval_inline.rs"]
pub mod approval_inline;
#[path = "widgets/chat_widget.rs"]
pub mod chat_widget;
#[path = "overlays/command_popup.rs"]
pub mod command_popup;
pub mod event;
#[path = "widgets/history_cell/mod.rs"]
pub mod history_cell;
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
#[path = "widgets/tool_activity/mod.rs"]
pub mod tool_activity;
#[path = "widgets/tool_rendering.rs"]
mod tool_rendering;
#[path = "overlays/user_input_popup.rs"]
pub mod user_input_popup;

use crate::tui::app::{App, ModelSwitchConfig, TuiLayoutSettings};
use crate::tui::event::Event;
use anyhow::Result;
use crossterm::event::{Event as CEvent, EventStream};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::stdout;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use telos_agent::{AgentConfig, MemoryStore, ModelProvider, ToolRegistry};

/// Ensures the terminal leaves raw mode and the alternate screen on panic or
/// early return.
struct TuiGuard;

impl Drop for TuiGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableBracketedPaste,
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen
        );
    }
}

/// Launch the ratatui full-screen TUI.
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
) -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let _ = crossterm::execute!(stdout, crossterm::event::EnableBracketedPaste);
    let _guard = TuiGuard;

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
    )?;
    let tick_rate = Duration::from_millis(100);
    let mut reader = EventStream::new();

    loop {
        terminal.draw(|frame| app.draw(frame))?;

        let event = tokio::select! {
            maybe_event = reader.next() => {
                match maybe_event {
                    Some(Ok(CEvent::Key(key))) => Event::Key(key),
                    Some(Ok(CEvent::Mouse(mouse))) => Event::Mouse(mouse),
                    Some(Ok(CEvent::Paste(text))) => Event::Paste(text),
                    Some(Ok(CEvent::Resize(cols, rows))) => Event::Resize { cols, rows },
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
