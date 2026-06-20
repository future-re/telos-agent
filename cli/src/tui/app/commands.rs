use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::tui::approval::PendingApproval;
use crate::tui::command_popup::SlashCommand;
use crate::tui::history_cell::{AgentCell, ErrorCell, UserCell};
use crate::tui::input_panel::InputEvent;
use crate::tui::overlay::Overlay;
use crate::tui::selection_popup::SelectionPopup;
use crate::tui::user_input_popup::{Question, UserInputPopup};

use super::background::BackgroundCommand;
use super::config::save_deepseek_api_key;
use super::{App, MODEL_OPTIONS, Mode};
use telos_agent::ApprovalDecision;

const DEEPSEEK_PRO_MODEL: &str = "deepseek-v4-pro";
const DEEPSEEK_FLASH_MODEL: &str = "deepseek-v4-flash";

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeepSeekSwitch {
    Auto { label: String },
    Single { model: String, label: String },
}

impl App {
    pub(super) fn enqueue_inline_approval(&mut self, pending: PendingApproval) {
        if self.inline_approval.is_none() {
            self.inline_approval = Some(pending);
            self.inline_approval_expanded = false;
        } else {
            self.inline_approval_queue.push_back(pending);
        }
    }

    pub(super) fn resolve_inline_approval(&mut self, decision: ApprovalDecision) {
        if let Some(mut pending) = self.inline_approval.take()
            && let Some(tx) = pending.respond.take()
            && tx.send(decision).is_err()
        {
            self.status_text = "approval response channel closed".to_string();
        }
        self.inline_approval = self.inline_approval_queue.pop_front();
        self.inline_approval_expanded = false;
    }

    pub(super) fn open_inline_approval_edit_popup(&mut self) {
        if let Some(pending) = self.inline_approval.take() {
            self.inline_approval_expanded = false;
            self.open_approval_edit_popup(pending);
        }
    }

    pub(super) fn toggle_inline_approval_expanded(&mut self) -> bool {
        if self.inline_approval.is_none() {
            return false;
        }
        self.inline_approval_expanded = !self.inline_approval_expanded;
        true
    }

    pub(super) fn inline_approval_command_contains_point(&self, column: u16, row: u16) -> bool {
        let Some(area) = self.inline_approval_area else {
            return false;
        };
        let Some(pending) = &self.inline_approval else {
            return false;
        };
        let command_lines = crate::tui::approval_inline::approval_lines(
            pending,
            area.width as usize,
            self.inline_approval_expanded,
        )
        .len()
        .saturating_sub(3);
        let command_start = area.y.saturating_add(2);
        let command_end = command_start.saturating_add(command_lines as u16);

        column >= area.x.saturating_add(1)
            && column < area.x.saturating_add(area.width.saturating_sub(1))
            && row >= command_start
            && row < command_end
    }

    pub(super) async fn handle_input_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::Submit(prompt) => {
                self.send_prompt(prompt).await;
            }
            InputEvent::SlashCommand(cmd) => {
                self.handle_slash_command(cmd).await;
            }
            InputEvent::None => {}
        }
    }

    async fn handle_slash_command(&mut self, cmd: SlashCommand) {
        match cmd {
            SlashCommand::Help => {
                let help_text = "\
Available commands:\n\n  /tool    — show registered tools and aliases\n\
  /model   — switch the model for later turns\n\
  /api     — set the DeepSeek API key\n\
  /session — new, list, or resume stored sessions\n\
  /tasks   — show persisted tasks\n\
  /clear   — clear the visible conversation\n\
  /auto    — toggle auto-approve mode\n\
  Ctrl+D   — quit when input is empty\n\
  /help    — show this help";
                self.chat.push_cell(Box::new(UserCell { content: format!("/{cmd:?}") }));
                self.chat.push_cell(Box::new(AgentCell {
                    buffer: help_text.to_string(),
                    is_streaming: false,
                }));
            }
            SlashCommand::Clear => {
                // App already has Ctrl+L for clear; /clear does the same
                self.chat.clear();
                self.tool_activity.clear();
                self.chat.scroll_to_bottom();
            }
            SlashCommand::Auto => {
                let on = !self.auto_mode.load(Ordering::Relaxed);
                self.auto_mode.store(on, Ordering::Relaxed);
                self.update_auto_mode_status();
            }
            SlashCommand::Model => {
                let popup = SelectionPopup::new(" Select model ", MODEL_OPTIONS.to_vec())
                    .with_context("model");
                self.overlays.push(Box::new(popup));
                self.mode = Mode::Approving;
            }
            SlashCommand::Api => {
                self.open_api_settings_popup();
            }
            SlashCommand::Tool => {
                self.show_tool_summary();
            }
            SlashCommand::Session => {
                let popup = SelectionPopup::new(
                    " Session ",
                    vec!["new session", "resume session", "list sessions"],
                )
                .with_context("session_action");
                self.overlays.push(Box::new(popup));
                self.mode = Mode::Approving;
            }
            SlashCommand::Tasks => {
                self.show_task_summary();
            }
        }
    }

    /// Process a popped overlay — extract results from selection popups, etc.
    pub(super) async fn handle_overlay_popped(&mut self, popped: Option<Box<dyn Overlay>>) {
        let Some(overlay) = popped else { return };
        if let Some(popup) = overlay.as_any().downcast_ref::<SelectionPopup>() {
            match popup.context() {
                Some("model") => {
                    if let Some(model) = popup.selected_item() {
                        self.switch_model(model);
                    }
                }
                Some("session_action") => {
                    if let Some(idx) = popup.selected_index() {
                        self.handle_session_action(idx).await;
                    }
                }
                Some("session_resume") => {
                    if let Some(session_id) = popup.selected_item() {
                        self.resume_session(session_id).await;
                    }
                }
                _ => {}
            }
            return;
        }

        if let Some(popup) = overlay.as_any().downcast_ref::<UserInputPopup>()
            && popup.context() == Some("approval_edit")
        {
            if let Some(answers) = popup.answers() {
                let edited = answers.get("arguments").cloned().unwrap_or_default();
                match serde_json::from_str::<serde_json::Value>(&edited) {
                    Ok(arguments) => {
                        if let Some(mut pending) = self.editing_approval.take()
                            && let Some(tx) = pending.respond.take()
                        {
                            let _ = tx.send(telos_agent::ApprovalDecision::Modify { arguments });
                        }
                    }
                    Err(err) => {
                        if let Some(pending) = self.editing_approval.take() {
                            self.open_approval_edit_popup_with_error(
                                pending,
                                edited,
                                format!("invalid JSON: {err}"),
                            );
                        }
                    }
                }
            } else if let Some(mut pending) = self.editing_approval.take()
                && let Some(tx) = pending.respond.take()
            {
                let _ = tx.send(telos_agent::ApprovalDecision::Deny {
                    reason: "modification cancelled".into(),
                });
            }
            return;
        }

        if let Some(popup) = overlay.as_any().downcast_ref::<UserInputPopup>()
            && popup.context() == Some("api_settings")
            && let Some(answers) = popup.answers()
        {
            let key = answers.get("deepseek_api_key").cloned().unwrap_or_default();
            self.set_deepseek_api_key(key);
        }
    }

    pub(super) fn refresh_mode_after_overlay(&mut self) {
        self.mode = if self.overlays.is_empty() {
            if self.turn_active { Mode::Streaming } else { Mode::Normal }
        } else {
            Mode::Approving
        };
    }

    pub(super) fn open_approval_edit_popup(&mut self, pending: PendingApproval) {
        let initial = serde_json::to_string_pretty(&pending.request.arguments)
            .unwrap_or_else(|_| pending.request.arguments.to_string());
        self.open_approval_edit_popup_with_error(pending, initial, String::new());
    }

    fn open_approval_edit_popup_with_error(
        &mut self,
        pending: PendingApproval,
        initial: String,
        error: String,
    ) {
        self.editing_approval = Some(pending);
        let mut popup = UserInputPopup::new(
            " Edit approval arguments ",
            vec![Question {
                key: "arguments".into(),
                label: "JSON arguments".into(),
                value: initial,
                placeholder: "{}".into(),
            }],
        )
        .with_context("approval_edit");
        if !error.is_empty() {
            popup.set_error(error);
        }
        self.overlays.push(Box::new(popup));
        self.mode = Mode::Approving;
    }

    fn open_api_settings_popup(&mut self) {
        let popup = UserInputPopup::new(
            " API settings ",
            vec![Question {
                key: "deepseek_api_key".into(),
                label: "DeepSeek API key".into(),
                value: String::new(),
                placeholder: String::new(),
            }],
        )
        .with_context("api_settings");
        self.overlays.push(Box::new(popup));
        self.mode = Mode::Approving;
    }

    fn set_deepseek_api_key(&mut self, key: String) {
        let key = key.trim().to_string();
        if key.is_empty() {
            self.chat.push_cell(Box::new(ErrorCell {
                message: "API key was empty; no changes saved".to_string(),
            }));
            return;
        }
        self.model_switch.deepseek_api_key = Some(key.clone());
        if let Some(base) = dirs::config_dir() {
            let path = base.join("telos").join("config.toml");
            if let Err(err) = save_deepseek_api_key(&path, &key) {
                self.chat.push_cell(Box::new(ErrorCell {
                    message: format!("failed to save API key: {err}"),
                }));
                return;
            }
        }
        self.switch_to_default_deepseek_provider(&key);
        self.status_text = "telos · API key configured".to_string();
        self.base_status = self.status_text.clone();
        self.chat.push_cell(Box::new(AgentCell {
            buffer: "DeepSeek API key configured and applied to the current session.".to_string(),
            is_streaming: false,
        }));
    }

    fn switch_to_default_deepseek_provider(&mut self, key: &str) {
        let config = telos_agent::RoutedModelConfig::dual(
            key.to_string(),
            DEEPSEEK_PRO_MODEL.to_string(),
            DEEPSEEK_FLASH_MODEL.to_string(),
        );
        let provider = Arc::new(telos_agent::RoutedProvider::new(config));
        let _ = self
            .turn_tx
            .send(BackgroundCommand::SetProvider { provider, label: "auto".to_string() });
    }

    fn switch_model(&mut self, model: &str) {
        let Some(api_key) = self.model_switch.deepseek_api_key.clone() else {
            self.chat.push_cell(Box::new(ErrorCell {
                message: "cannot switch model: missing DeepSeek API key".to_string(),
            }));
            return;
        };

        let selected = deepseek_switch_for_model_choice(model);
        let label = match selected {
            DeepSeekSwitch::Auto { ref label } => {
                let config = telos_agent::RoutedModelConfig::dual(
                    api_key,
                    DEEPSEEK_PRO_MODEL.to_string(),
                    DEEPSEEK_FLASH_MODEL.to_string(),
                );
                let provider = Arc::new(telos_agent::RoutedProvider::new(config));
                let _ = self
                    .turn_tx
                    .send(BackgroundCommand::SetProvider { provider, label: label.clone() });
                label.clone()
            }
            DeepSeekSwitch::Single { ref model, ref label } => {
                let provider = Arc::new(telos_agent::DeepSeekProvider::new(
                    telos_agent::DeepSeekConfig::new(api_key, model.clone()),
                ));
                let _ = self
                    .turn_tx
                    .send(BackgroundCommand::SetProvider { provider, label: label.clone() });
                label.clone()
            }
        };

        self.status_text = format!("telos · model {label}");
        self.base_status = self.status_text.clone();
        self.chat.push_cell(Box::new(UserCell { content: format!("/model {label}") }));
        self.chat.push_cell(Box::new(AgentCell {
            buffer: format!("Switched model to: {label}"),
            is_streaming: false,
        }));
    }
}

fn deepseek_switch_for_model_choice(choice: &str) -> DeepSeekSwitch {
    let choice = choice.trim();
    if choice.eq_ignore_ascii_case("auto") {
        return DeepSeekSwitch::Auto { label: "auto".into() };
    }
    if choice.eq_ignore_ascii_case("pro") {
        return DeepSeekSwitch::Single { model: DEEPSEEK_PRO_MODEL.into(), label: "pro".into() };
    }
    if choice.eq_ignore_ascii_case("flash") {
        return DeepSeekSwitch::Single {
            model: DEEPSEEK_FLASH_MODEL.into(),
            label: "flash".into(),
        };
    }
    DeepSeekSwitch::Single { model: choice.to_string(), label: choice.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_choice_auto_keeps_routed_mode() {
        assert_eq!(
            deepseek_switch_for_model_choice("auto"),
            DeepSeekSwitch::Auto { label: "auto".into() }
        );
    }

    #[test]
    fn model_choice_pro_uses_full_pro_model() {
        assert_eq!(
            deepseek_switch_for_model_choice("pro"),
            DeepSeekSwitch::Single { model: DEEPSEEK_PRO_MODEL.into(), label: "pro".into() }
        );
    }
}
