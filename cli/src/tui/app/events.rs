use std::sync::atomic::Ordering;

use crate::tui::event::{AppEvent, Event};
use crate::tui::history_cell::{ErrorCell, SeparatorCell};
use crate::tui::input_panel::InputMode;
use crate::tui::overlay::{ApprovalOverlay, OverlayAction};

use super::{App, Mode};

impl App {
    /// Process a single event.
    pub async fn handle_event(&mut self, event: Event) -> anyhow::Result<()> {
        match event {
            Event::Key(key) => {
                use crossterm::event::{KeyCode, KeyModifiers};

                // Global shortcuts.
                match (key.code, key.modifiers) {
                    (KeyCode::Char('d'), KeyModifiers::CONTROL) if self.input.is_empty() => {
                        self.should_quit = true;
                        return Ok(());
                    }
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        self.cancellation.cancel();
                        self.turn_active = false;
                        self.turn_started = None;
                        self.status_text = self.base_status.clone();
                        return Ok(());
                    }
                    (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                        self.chat.clear();
                        self.tool_activity.clear();
                        self.chat.scroll_to_bottom();
                        return Ok(());
                    }
                    (KeyCode::BackTab, _) => {
                        let on = !self.auto_mode.load(Ordering::Relaxed);
                        self.auto_mode.store(on, Ordering::Relaxed);
                        self.update_auto_mode_status();
                        return Ok(());
                    }
                    (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                        self.new_session();
                        return Ok(());
                    }
                    _ => {}
                }

                match self.mode {
                    Mode::Approving => {
                        if let Some(overlay) = self.overlays.last_mut() {
                            match overlay.handle_key(key) {
                                OverlayAction::Pop => {
                                    let popped = self.overlays.pop();
                                    self.handle_overlay_popped(popped).await;
                                    self.refresh_mode_after_overlay();
                                }
                                OverlayAction::Handled => {
                                    if let Some(approval) =
                                        overlay.as_any_mut().downcast_mut::<ApprovalOverlay>()
                                        && let Some(pending) = approval.take_edit_request()
                                    {
                                        let _ = self.overlays.pop();
                                        self.open_approval_edit_popup(pending);
                                        self.mode = Mode::Approving;
                                    }
                                }
                                OverlayAction::None => {}
                            }
                        }
                        return Ok(());
                    }
                    Mode::Normal => {
                        if self.input.input_mode() != InputMode::Normal {
                            let input_event = self.input.handle_key(key);
                            self.handle_input_event(input_event).await;
                            return Ok(());
                        }

                        // Scroll keys
                        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                        match (key.code, ctrl) {
                            (KeyCode::PageUp, _) => {
                                self.chat.scroll_up(10);
                                return Ok(());
                            }
                            (KeyCode::PageDown, _) => {
                                self.chat.scroll_down(10);
                                return Ok(());
                            }
                            (KeyCode::Up, false) => {
                                if self.input.wants_vertical_nav_key(key) {
                                    let input_event = self.input.handle_key(key);
                                    self.handle_input_event(input_event).await;
                                } else {
                                    self.chat.scroll_up(1);
                                }
                                return Ok(());
                            }
                            (KeyCode::Down, false) => {
                                if self.input.wants_vertical_nav_key(key) {
                                    let input_event = self.input.handle_key(key);
                                    self.handle_input_event(input_event).await;
                                } else {
                                    self.chat.scroll_down(1);
                                }
                                return Ok(());
                            }
                            (KeyCode::Tab, _) => {
                                if self.tool_activity.is_empty() {
                                    self.chat.select_next_tool();
                                } else {
                                    self.tool_activity.select_next();
                                }
                                return Ok(());
                            }
                            (KeyCode::BackTab, _) => {
                                if self.tool_activity.is_empty() {
                                    self.chat.select_prev_tool();
                                } else {
                                    self.tool_activity.select_prev();
                                }
                                return Ok(());
                            }
                            (KeyCode::Char('t'), true)
                                if self.tool_activity.toggle_selected()
                                    || self.chat.toggle_selected_tool() =>
                            {
                                return Ok(());
                            }
                            _ => {}
                        }

                        // Input handling with InputEvent
                        let input_event = self.input.handle_key(key);
                        self.handle_input_event(input_event).await;
                    }
                    Mode::Streaming => {
                        // During streaming, only scroll keys are handled.
                        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                        match (key.code, ctrl) {
                            (KeyCode::PageUp, _) => self.chat.scroll_up(10),
                            (KeyCode::PageDown, _) => self.chat.scroll_down(10),
                            (KeyCode::Up, false) => self.chat.scroll_up(1),
                            (KeyCode::Down, false) => self.chat.scroll_down(1),
                            (KeyCode::Tab, _) => {
                                if self.tool_activity.is_empty() {
                                    self.chat.select_next_tool();
                                } else {
                                    self.tool_activity.select_next();
                                }
                            }
                            (KeyCode::BackTab, _) => {
                                if self.tool_activity.is_empty() {
                                    self.chat.select_prev_tool();
                                } else {
                                    self.tool_activity.select_prev();
                                }
                            }
                            (KeyCode::Enter, _)
                            | (KeyCode::Char(' '), _)
                            | (KeyCode::Char('t'), true) => {
                                let _ = self.tool_activity.toggle_selected()
                                    || self.chat.toggle_selected_tool();
                            }
                            _ => {}
                        }
                    }
                }
            }
            Event::Tick => {
                self.spinner_frame = self.spinner_frame.wrapping_add(1);
                while let Ok(event) = self.turn_rx.try_recv() {
                    match event {
                        Event::Turn(turn_event) => self.handle_turn_event(turn_event).await,
                        Event::TurnComplete => {
                            self.push_turn_summary();
                            self.chat.push_cell(Box::new(SeparatorCell));
                            self.mode = Mode::Normal;
                            self.turn_active = false;
                            self.turn_started = None;
                            self.reset_turn_usage();
                            self.turn_tool_calls = 0;
                            self.turn_tool_failures = 0;
                            self.status_text = self.base_status.clone();
                        }
                        Event::SessionError { message } => {
                            self.chat.push_cell(Box::new(ErrorCell { message }));
                            self.mode = Mode::Normal;
                            self.turn_active = false;
                            self.turn_started = None;
                            self.reset_turn_usage();
                            self.turn_tool_calls = 0;
                            self.turn_tool_failures = 0;
                            self.status_text = self.base_status.clone();
                        }
                        Event::SessionNotice { message } => {
                            self.status_text = format!("telos · {message}");
                            self.base_status = self.status_text.clone();
                        }
                        _ => {}
                    }
                }
                while let Ok(pending) = self.approval_rx.try_recv() {
                    self.overlays.push(Box::new(ApprovalOverlay::new(pending)));
                    self.mode = Mode::Approving;
                }
                // ── Process internal event bus ────────────────────────
                while let Ok(app_event) = self.app_event_rx.try_recv() {
                    match app_event {
                        AppEvent::StatusChanged(text) => {
                            self.status_text = text;
                        }
                        AppEvent::TokenUsage { used, max } => {
                            if !self.turn_has_provider_usage {
                                self.turn_input_tokens = used;
                            }
                            self.token_budget_max = Some(max);
                        }
                        AppEvent::ConfigChanged(key) => {
                            tracing::debug!("config changed: {key}");
                        }
                    }
                }
            }
            Event::Resize { .. } => {}
            Event::Mouse(_) => {}
            // Turn, TurnComplete, and SessionError are only received via
            // turn_rx.try_recv() inside the Tick handler above and never
            // arrive at the outer match from the main event loop.
            _ => {}
        }
        Ok(())
    }
}
