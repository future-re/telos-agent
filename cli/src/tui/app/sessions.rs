use telos_agent::Role;

use super::background::BackgroundCommand;
use super::session_list::session_ids_in_dir;
use super::tasks::{format_task_summary, tasks_in_dir};
use super::{App, Mode};
use crate::tui::chat_entry::ChatEntry;
use crate::tui::selection_popup::SelectionPopup;

impl App {
    pub(super) async fn handle_session_action(&mut self, idx: usize) {
        match idx {
            0 => self.new_session(),
            1 => self.open_session_resume_popup(),
            2 => self.show_session_list(),
            _ => {}
        }
    }

    pub(super) fn new_session(&mut self) {
        if self.turn_active {
            self.chat.push_entry(ChatEntry::error(
                "wait for the current turn before starting a new session".to_string(),
            ));
            return;
        }
        // Deny any pending approvals so the background agent doesn't hang.
        self.clear_pending_approvals();
        self.overlays.clear();
        self.editing_approval = None;
        self.chat.clear();
        self.input.clear_history();
        self.reset_turn_usage();
        self.turn_tool_calls = 0;
        self.turn_tool_failures = 0;
        self.turn_started = None;
        self.cancellation.reset();
        let _ = self.turn_tx.send(BackgroundCommand::NewSession);
        self.status_text = "telos · new session".to_string();
        self.base_status = self.status_text.clone();
    }

    fn open_session_resume_popup(&mut self) {
        let sessions = self.session_ids();
        if sessions.is_empty() {
            self.chat.push_entry(ChatEntry::agent("No saved sessions found.".to_string(), false));
            return;
        }
        self.overlays.push(Box::new(
            SelectionPopup::new(" Resume session ", sessions).with_context("session_resume"),
        ));
        self.mode = Mode::Approving;
    }

    fn show_session_list(&mut self) {
        let sessions = self.session_ids();
        let body = if sessions.is_empty() {
            "No saved sessions found.".to_string()
        } else {
            format!(
                "Saved sessions:\n\n{}",
                sessions.into_iter().map(|s| format!("  {s}")).collect::<Vec<_>>().join("\n")
            )
        };
        self.chat.push_entry(ChatEntry::agent(body, false));
    }

    pub(super) fn show_task_summary(&mut self) {
        let tasks = tasks_in_dir(&self.task_dir);
        self.chat.push_entry(ChatEntry::agent(format_task_summary(&tasks), false));
    }

    pub(super) async fn resume_session(&mut self, session_id: &str) {
        if self.turn_active {
            self.chat.push_entry(ChatEntry::error(
                "wait for the current turn before resuming a session".to_string(),
            ));
            return;
        }
        self.clear_pending_approvals();
        self.overlays.clear();
        self.editing_approval = None;
        match self.storage.load(session_id).await {
            Ok(messages) => {
                let input_history = messages
                    .iter()
                    .filter(|message| message.role == Role::User)
                    .map(telos_agent::Message::text_content)
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>();
                self.chat.clear();
                self.input.replace_history(input_history);
                self.chat
                    .push_entry(ChatEntry::agent(format!("Resumed session: {session_id}"), false));
                for message in messages {
                    self.push_message_entry(message);
                }
                self.cancellation.reset();
                let _ = self.turn_tx.send(BackgroundCommand::ResumeSession(session_id.to_string()));
                self.status_text = format!("telos · session {session_id}");
                self.base_status = self.status_text.clone();
            }
            Err(err) => self.chat.push_entry(ChatEntry::error(format!(
                "failed to load session {session_id}: {err}"
            ))),
        }
    }

    fn push_message_entry(&mut self, message: telos_agent::Message) {
        match message.role {
            Role::System => {}
            Role::User => {
                let text = message.text_content();
                if !text.is_empty() {
                    self.chat.push_entry(ChatEntry::user(text));
                }
            }
            Role::Assistant => {
                let thinking = message.thinking_content();
                if !thinking.is_empty() {
                    self.chat.push_entry(ChatEntry::thinking(thinking, false));
                }
                let text = message.text_content();
                if !text.is_empty() {
                    self.chat.push_entry(ChatEntry::agent(text, false));
                }
            }
            Role::Tool => {
                for result in message.tool_results_iter() {
                    let mut entry = ChatEntry::tool_call(
                        result.tool_call_id.clone(),
                        result.name.clone(),
                        result.content.to_string(),
                    );
                    entry.set_completed(!result.is_error);
                    self.chat.push_entry(entry);
                }
            }
        }
    }

    fn session_ids(&self) -> Vec<String> {
        session_ids_in_dir(&self.sessions_dir)
    }

    pub(super) fn show_tool_summary(&mut self) {
        if self.tool_infos.is_empty() {
            self.chat.push_entry(ChatEntry::agent("No tools are registered.".to_string(), false));
            return;
        }
        let mut lines = Vec::new();
        lines.push("Registered tools:".to_string());
        lines.push(String::new());
        for tool in &self.tool_infos {
            let aliases = if tool.aliases.is_empty() {
                "no aliases".to_string()
            } else {
                format!("aliases: {}", tool.aliases.join(", "))
            };
            lines.push(format!("  {} ({})", tool.name, aliases));
            if !tool.description.is_empty() {
                lines.push(format!("    {}", tool.description));
            }
        }
        self.chat.push_entry(ChatEntry::agent(lines.join("\n"), false));
    }
}
