use std::sync::atomic::Ordering;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::event::{AppEvent, Event};
use crate::tui::history_cell::{ErrorCell, SeparatorCell};
use crate::tui::input_panel::InputMode;
use crate::tui::keymap::{is_ctrl_char, is_shift_tab};
use crate::tui::overlay::{ApprovalOverlay, OverlayAction};

use super::{App, Mode};

impl App {
    /// Process a single event.
    pub async fn handle_event(&mut self, event: Event) -> anyhow::Result<()> {
        match event {
            Event::Key(key) => {
                // Global shortcuts.
                if is_ctrl_char(key, 'd') && self.input.is_empty() {
                    self.should_quit = true;
                    return Ok(());
                }
                if is_ctrl_char(key, 'c') {
                    if self.turn_active {
                        self.cancellation.cancel();
                        self.chat.finish_streaming_cells();
                        self.mode = Mode::Normal;
                        self.input.clear();
                        self.status_text = "cancelling…".to_string();
                    }
                    return Ok(());
                }
                if is_ctrl_char(key, 'l') {
                    self.chat.clear();
                    self.tool_activity.clear();
                    self.chat.scroll_to_bottom();
                    return Ok(());
                }
                if is_ctrl_char(key, 'n') {
                    self.new_session();
                    return Ok(());
                }

                if is_shift_tab(key) {
                    let on = !self.auto_mode.load(Ordering::Relaxed);
                    self.auto_mode.store(on, Ordering::Relaxed);
                    self.update_auto_mode_status();
                    return Ok(());
                }

                if self.inline_approval.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('a') | KeyCode::Char('y'), KeyModifiers::NONE) => {
                            self.resolve_inline_approval(telos_agent::ApprovalDecision::Allow);
                            return Ok(());
                        }
                        (KeyCode::Char('d') | KeyCode::Char('n'), KeyModifiers::NONE) => {
                            self.resolve_inline_approval(telos_agent::ApprovalDecision::Deny {
                                reason: "denied by user".into(),
                            });
                            return Ok(());
                        }
                        (KeyCode::Char('e'), KeyModifiers::NONE) => {
                            self.open_inline_approval_edit_popup();
                            return Ok(());
                        }
                        (KeyCode::Char('t') | KeyCode::Char(' '), KeyModifiers::NONE) => {
                            self.toggle_inline_approval_expanded();
                            return Ok(());
                        }
                        _ => {}
                    }
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

                        let input_event = self.input.handle_key(key);
                        self.handle_input_event(input_event).await;
                    }
                    Mode::Streaming => {
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
                                self.chat.scroll_up(1);
                                return Ok(());
                            }
                            (KeyCode::Down, false) => {
                                self.chat.scroll_down(1);
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
                            (KeyCode::Char(' '), _) | (KeyCode::Char('t'), true) => {
                                let _ = self.tool_activity.toggle_selected()
                                    || self.chat.toggle_selected_tool();
                                return Ok(());
                            }
                            _ => {}
                        }

                        let input_event = self.input.handle_key(key);
                        self.handle_input_event(input_event).await;
                    }
                }
            }
            Event::Tick => {
                self.spinner_frame = self.spinner_frame.wrapping_add(1);
                while let Ok(event) = self.turn_rx.try_recv() {
                    match event {
                        Event::Turn(turn_event) => self.handle_turn_event(turn_event).await,
                        Event::TurnComplete => {
                            if self.has_visible_turn_activity() {
                                self.finalize_turn_ui();
                            }
                            self.reset_turn_state();
                        }
                        Event::SessionError { message } => {
                            self.chat.finish_streaming_cells();
                            if message != "cancelled" {
                                self.chat.push_cell(Box::new(ErrorCell { message }));
                            }
                            if self.has_visible_turn_activity() {
                                self.push_turn_summary();
                                self.chat.push_cell(Box::new(SeparatorCell));
                            }
                            self.reset_turn_state();
                        }
                        Event::SessionNotice { message } => {
                            self.status_text = format!("telos · {message}");
                            self.base_status = self.status_text.clone();
                        }
                        _ => {}
                    }
                }
                while let Ok(pending) = self.approval_rx.try_recv() {
                    self.enqueue_inline_approval(pending);
                }
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
            Event::Paste(text) => {
                self.input.insert_text(&text);
            }
            Event::Mouse(mouse) => {
                use crossterm::event::{MouseButton, MouseEventKind};

                match mouse.kind {
                    MouseEventKind::ScrollUp => self.chat.scroll_up(1),
                    MouseEventKind::ScrollDown => self.chat.scroll_down(1),
                    MouseEventKind::Down(MouseButton::Left)
                        if self.inline_approval_command_contains_point(mouse.column, mouse.row) =>
                    {
                        self.toggle_inline_approval_expanded();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }
}
