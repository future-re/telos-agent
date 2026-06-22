//! Terminal event loop — raw mode, event stream, draw loop.

use std::io::stdout;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    Event as CEvent, EventStream, KeyEventKind, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use crate::tui::app::{App, AppEvent};
use crate::tui::turn::{TurnCommand, TurnUpdate, apply_update};

/// Ensures terminal cleanup on drop.
struct TuiGuard {
    kb_enhanced: bool,
}

impl Drop for TuiGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        if self.kb_enhanced {
            let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableBracketedPaste,
            crossterm::terminal::LeaveAlternateScreen
        );
    }
}

/// Launch the TUI with the given agent session.
pub async fn run(
    status_text: String,
    auto_mode: Arc<AtomicBool>,
    mut turn_rx: mpsc::UnboundedReceiver<TurnUpdate>,
    turn_tx: mpsc::UnboundedSender<TurnCommand>,
) -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut guard = TuiGuard { kb_enhanced: false };

    let mut stdout = stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    guard.kb_enhanced = matches!(crossterm::terminal::supports_keyboard_enhancement(), Ok(true))
        && crossterm::execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok();
    let _ = crossterm::execute!(stdout, crossterm::event::EnableBracketedPaste);

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(status_text, auto_mode);
    let tick_rate = Duration::from_millis(100);
    let mut reader = EventStream::new();

    loop {
        // ── Draw ──────────────────────────────────────────────────────
        terminal.draw(|frame| app.draw(frame.area(), frame.buffer_mut()))?;

        // ── Wait for next event ───────────────────────────────────────
        let event = tokio::select! {
            maybe = reader.next() => {
                match maybe {
                    Some(Ok(event)) => crossterm_to_app(event),
                    _ => continue,
                }
            }
            _ = tokio::time::sleep(tick_rate) => AppEvent::Tick,
        };

        // ── Handle turn updates ───────────────────────────────────────
        while let Ok(update) = turn_rx.try_recv() {
            match &update {
                TurnUpdate::StatusText(text) => app.status_text = text.clone(),
                TurnUpdate::Completed => app.turn_active = false,
                _ => {}
            }
            apply_update(&mut app.chat, update);
        }

        // ── Handle app event ──────────────────────────────────────────
        match event {
            AppEvent::Key(key) => {
                // If approval overlay is visible, route keys there.
                if app.approval.is_visible() {
                    if let crossterm::event::KeyCode::Char(c) = key.code
                        && let Some(_decision) = app.approval.handle_key(c)
                    {
                        // Forward decision to turn. For now just clear.
                        app.approval.request = None;
                    }
                    continue;
                }

                // Enter submits the composer.
                if key.code == crossterm::event::KeyCode::Enter && key.modifiers.is_empty() {
                    let text = app.composer.text();
                    if !text.trim().is_empty() {
                        let _ = turn_tx.send(TurnCommand::Prompt(text.clone()));
                        app.handle_event(AppEvent::Submit(text));
                    }
                    continue;
                }

                app.handle_event(AppEvent::Key(key));
            }
            AppEvent::Paste(text) => app.handle_event(AppEvent::Paste(text)),
            AppEvent::Tick => app.handle_event(AppEvent::Tick),
            AppEvent::Mouse(mouse) => {
                use crossterm::event::MouseEventKind;
                match mouse.kind {
                    MouseEventKind::ScrollUp => app.chat.scroll_up(1),
                    MouseEventKind::ScrollDown => app.chat.scroll_down(1),
                    _ => {}
                }
            }
            _ => {}
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn crossterm_to_app(event: CEvent) -> AppEvent {
    match event {
        CEvent::Key(key) if key.kind == KeyEventKind::Release => AppEvent::Tick,
        CEvent::Key(key) => AppEvent::Key(key),
        CEvent::Mouse(mouse) => AppEvent::Mouse(mouse),
        CEvent::Paste(text) => AppEvent::Paste(text),
        CEvent::Resize(..) => AppEvent::Resize,
        _ => AppEvent::Tick,
    }
}
