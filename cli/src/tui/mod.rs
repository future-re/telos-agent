pub mod app;
pub mod approval;
pub mod chat_panel;
pub mod event;
pub mod input_panel;
pub mod markdown;
pub mod status_bar;
pub mod theme;

use crate::tui::app::App;
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
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
    }
}

/// Launch the ratatui full-screen TUI.
pub async fn run(
    config: AgentConfig,
    provider: Arc<dyn ModelProvider>,
    tools: ToolRegistry,
    status_text: String,
    project_root: Option<&std::path::Path>,
    auto_mode: bool,
    memory_store: Arc<Mutex<MemoryStore>>,
) -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let _guard = TuiGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app =
        App::new(config, provider, tools, status_text, project_root, auto_mode, memory_store)?;
    let tick_rate = Duration::from_millis(100);
    let mut reader = EventStream::new();

    loop {
        terminal.draw(|frame| app.draw(frame))?;

        let event = tokio::select! {
            maybe_event = reader.next() => {
                match maybe_event {
                    Some(Ok(CEvent::Key(key))) => Event::Key(key),
                    Some(Ok(CEvent::Mouse(mouse))) => Event::Mouse(mouse),
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
