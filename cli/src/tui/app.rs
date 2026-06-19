use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use telos_agent::{MemoryStore, Role, Storage, TurnEvent};
use tokio::sync::mpsc;

use crate::tui::approval::PendingApproval;
use crate::tui::chat_widget::ChatWidget;
use crate::tui::command_popup::SlashCommand;
use crate::tui::event::{AppEvent, Event};
use crate::tui::history_cell::*;
use crate::tui::input_panel::{InputEvent, InputMode, InputPanel};
use crate::tui::overlay::{ApprovalOverlay, Overlay, OverlayAction};
use crate::tui::selection_popup::SelectionPopup;
use crate::tui::status_bar;
use crate::tui::theme::Theme;
use crate::tui::tool_activity::ToolActivityPanel;
use crate::tui::user_input_popup::{Question, UserInputPopup};

const MODEL_OPTIONS: [&str; 2] = ["deepseek-v4-flash", "deepseek-v4-pro"];

#[derive(Debug, Clone, Default)]
pub struct ModelSwitchConfig {
    pub deepseek_api_key: Option<String>,
}

enum BackgroundCommand {
    Prompt(String),
    SetProvider { provider: Arc<dyn telos_agent::ModelProvider>, label: String },
    NewSession,
    ResumeSession(String),
}

#[derive(Clone)]
struct ToolInfo {
    name: String,
    aliases: Vec<String>,
    description: String,
}

/// TUI application mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Waiting for user input.
    Normal,
    /// Agent is streaming a response.
    Streaming,
    /// Approval overlay is visible.
    Approving,
}

/// Root application state for the TUI.
pub struct App {
    /// Current UI mode.
    pub mode: Mode,
    /// Whether the application should exit.
    pub should_quit: bool,
    /// Status text shown in the top bar.
    pub status_text: String,
    /// Chat widget (rendering + scrolling).
    pub chat: ChatWidget,
    /// Input panel at the bottom.
    pub input: InputPanel,
    /// Compact tool/command activity shown above the input panel.
    pub tool_activity: ToolActivityPanel,
    /// Active overlay stack (topmost overlay rendered last).
    pub overlays: Vec<Box<dyn Overlay>>,
    /// Whether a background turn is currently running.
    pub turn_active: bool,
    /// Saved base status text — restored after each turn.
    base_status: String,
    /// Shared cancellation state — set by Ctrl+C and read by the background task.
    cancellation: telos_agent::CancellationState,
    /// Auto-approve mode — toggle with Shift+Tab.
    auto_mode: Arc<AtomicBool>,
    /// When the current turn started (for elapsed display).
    turn_started: Option<Instant>,
    /// Tokens consumed in the current turn.
    turn_input_tokens: u64,
    turn_output_tokens: u64,
    /// Tool usage counters for the current turn.
    turn_tool_calls: u64,
    turn_tool_failures: u64,
    /// Spinner animation frame (incremented on Tick).
    spinner_frame: usize,
    /// Maximum tokens for the budget progress bar.
    token_budget_max: Option<u64>,
    /// Shared memory store for tools, prompt injection, and automatic feedback.
    memory: Arc<Mutex<MemoryStore>>,
    /// Session storage used by the background session and TUI resume UI.
    storage: Arc<dyn Storage>,
    /// On-disk directory containing JSONL sessions.
    sessions_dir: PathBuf,
    /// Model switch settings.
    model_switch: ModelSwitchConfig,
    /// Snapshot of registered tool metadata for /tool.
    tool_infos: Vec<ToolInfo>,
    /// Approval request currently being edited in a UserInputPopup.
    editing_approval: Option<PendingApproval>,
    /// Send commands to the background agent task.
    turn_tx: mpsc::UnboundedSender<BackgroundCommand>,
    /// Receive TurnEvents from the background agent task.
    turn_rx: mpsc::UnboundedReceiver<Event>,
    /// Receive pending approvals from the TuiApprovalHandler.
    approval_rx: mpsc::UnboundedReceiver<PendingApproval>,
    /// Sender side of the internal event bus — cloned and shared with sub-components.
    pub app_event_tx: mpsc::UnboundedSender<AppEvent>,
    /// Receiver side of the internal event bus.
    app_event_rx: mpsc::UnboundedReceiver<AppEvent>,
}

impl App {
    pub fn new(
        mut config: telos_agent::AgentConfig,
        provider: Arc<dyn telos_agent::ModelProvider>,
        tools: telos_agent::ToolRegistry,
        status_text: String,
        project_root: Option<&std::path::Path>,
        auto_mode_on: bool,
        memory: Arc<Mutex<MemoryStore>>,
        model_switch: ModelSwitchConfig,
    ) -> Result<Self, telos_agent::AgentError> {
        // Wire up session storage before creating the AgentSession.
        let session_manager = crate::session::SessionManager::new(project_root);
        std::fs::create_dir_all(session_manager.sessions_dir()).ok();
        let sessions_dir = session_manager.sessions_dir().to_path_buf();
        let storage =
            Arc::new(telos_agent::JsonlStorage::new(session_manager.sessions_dir().to_path_buf())?);
        config.storage = Some(storage.clone());
        let app_storage: Arc<dyn Storage> = storage.clone();

        // Extract cancellation state before moving config into the spawned task.
        let cancellation = config.cancellation.clone();
        let token_budget_max = config.token_budget.as_ref().map(|b| b.max_tokens as u64);

        // Auto-approve mode — shared between UI and approval handler.
        let auto_mode = Arc::new(AtomicBool::new(auto_mode_on));
        let auto_mode_bg = Arc::clone(&auto_mode);

        // Seed status text with auto tag if starting in auto mode.
        let status_text =
            if auto_mode_on { format!("{status_text} ⏵⏵ auto") } else { status_text };

        let tool_infos = collect_tool_infos(&tools);

        let (prompt_tx, mut prompt_rx) = mpsc::unbounded_channel::<BackgroundCommand>();
        let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();
        let (approval_tx, approval_rx) = mpsc::unbounded_channel::<PendingApproval>();
        let (app_event_tx, app_event_rx) = mpsc::unbounded_channel::<AppEvent>();

        // Background task owns the AgentSession because run_turn_stream needs &mut self.
        tokio::spawn(async move {
            let approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>> = Some(Arc::new(
                crate::tui::approval::TuiApprovalHandler::new(approval_tx, auto_mode_bg),
            ));
            let mut config = config;
            config.approval_handler = approval_handler;
            let base_config = config.clone();
            let storage_for_resume = storage.clone();

            let mut session = match telos_agent::AgentSession::new(config) {
                Ok(s) => s,
                Err(e) => {
                    // Surface the error so the TUI can display it.
                    let _ = event_tx.send(Event::SessionError { message: e.to_string() });
                    let _ = event_tx.send(Event::TurnComplete);
                    return;
                }
            };
            let mut current_provider = provider;

            while let Some(command) = prompt_rx.recv().await {
                match command {
                    BackgroundCommand::Prompt(prompt) => {
                        let erased = telos_agent::ErasedProvider(current_provider.as_ref());
                        {
                            let mut stream =
                                pin!(session.run_turn_stream(&erased, &tools, prompt,));
                            while let Some(event) = stream.next().await {
                                match event {
                                    Ok(te) => {
                                        let _ = event_tx.send(Event::Turn(te));
                                    }
                                    Err(e) => {
                                        let _ = event_tx
                                            .send(Event::SessionError { message: e.to_string() });
                                        break;
                                    }
                                }
                            }
                        }
                        let _ = session.save().await;
                        let _ = event_tx.send(Event::TurnComplete);
                    }
                    BackgroundCommand::SetProvider { provider, label } => {
                        current_provider = provider;
                        let _ = event_tx.send(Event::SessionNotice {
                            message: format!("model switched to {label}"),
                        });
                    }
                    BackgroundCommand::NewSession => {
                        session = match telos_agent::AgentSession::new(base_config.clone()) {
                            Ok(s) => s,
                            Err(e) => {
                                let _ =
                                    event_tx.send(Event::SessionError { message: e.to_string() });
                                continue;
                            }
                        };
                        let _ = event_tx.send(Event::SessionNotice {
                            message: "new session started".to_string(),
                        });
                    }
                    BackgroundCommand::ResumeSession(session_id) => {
                        session = match telos_agent::AgentSession::resume(
                            session_id.clone(),
                            base_config.clone(),
                            storage_for_resume.clone(),
                        )
                        .await
                        {
                            Ok(s) => s,
                            Err(e) => {
                                let _ =
                                    event_tx.send(Event::SessionError { message: e.to_string() });
                                continue;
                            }
                        };
                        let _ = event_tx.send(Event::SessionNotice {
                            message: format!("resumed session {session_id}"),
                        });
                    }
                }
            }
        });

        Ok(Self {
            mode: Mode::Normal,
            should_quit: false,
            status_text: status_text.clone(),
            base_status: status_text,
            chat: ChatWidget::new(),
            input: InputPanel::new(),
            tool_activity: ToolActivityPanel::new(),
            overlays: Vec::new(),
            turn_active: false,
            cancellation,
            auto_mode,
            turn_started: None,
            turn_input_tokens: 0,
            turn_output_tokens: 0,
            turn_tool_calls: 0,
            turn_tool_failures: 0,
            memory,
            storage: app_storage,
            sessions_dir,
            model_switch,
            tool_infos,
            editing_approval: None,
            spinner_frame: 0,
            token_budget_max,
            turn_tx: prompt_tx,
            turn_rx: event_rx,
            approval_rx,
            app_event_tx,
            app_event_rx,
        })
    }

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
                            self.turn_input_tokens = 0;
                            self.turn_output_tokens = 0;
                            self.turn_tool_calls = 0;
                            self.turn_tool_failures = 0;
                            self.status_text = self.base_status.clone();
                        }
                        Event::SessionError { message } => {
                            self.chat.push_cell(Box::new(ErrorCell { message }));
                            self.mode = Mode::Normal;
                            self.turn_active = false;
                            self.turn_started = None;
                            self.turn_input_tokens = 0;
                            self.turn_output_tokens = 0;
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
                            self.turn_input_tokens = used;
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

    fn format_elapsed(&self) -> String {
        match self.turn_started {
            Some(start) => {
                let secs = start.elapsed().as_secs();
                if secs < 60 {
                    format!("{}s", secs)
                } else {
                    format!("{}m{}s", secs / 60, secs % 60)
                }
            }
            None => String::new(),
        }
    }

    fn format_token_usage(&self) -> String {
        let total = self.turn_input_tokens + self.turn_output_tokens;
        if total == 0 {
            return String::new();
        }
        let up_k = self.turn_input_tokens as f64 / 1000.0;
        let down_k = self.turn_output_tokens as f64 / 1000.0;
        format!("↑{:.1}k ↓{:.1}k", up_k, down_k)
    }

    fn push_turn_summary(&mut self) {
        let Some(summary) = self.format_turn_summary() else { return };
        self.chat.push_cell(Box::new(TurnSummaryCell { content: summary }));
    }

    fn format_turn_summary(&self) -> Option<String> {
        let has_activity = self.turn_started.is_some()
            || self.turn_tool_calls > 0
            || self.turn_input_tokens > 0
            || self.turn_output_tokens > 0;
        if !has_activity {
            return None;
        }

        let elapsed = self
            .turn_started
            .map(|started| format_duration_ms(started.elapsed().as_millis() as u64))
            .unwrap_or_else(|| "n/a".to_string());
        let tool_text = match (self.turn_tool_calls, self.turn_tool_failures) {
            (0, _) => "0 tools".to_string(),
            (calls, 0) => format!("{calls} tools"),
            (calls, failures) => format!("{calls} tools · {failures} failed"),
        };
        let token_text = format_turn_tokens(self.turn_input_tokens, self.turn_output_tokens);

        Some(format!("Turn {elapsed} · {tool_text} · {token_text}"))
    }

    /// Update status bar to reflect auto-mode state.
    fn update_auto_mode_status(&mut self) {
        let on = self.auto_mode.load(Ordering::Relaxed);
        let tag = " ⏵⏵ auto";
        let base = self.status_text.trim_end_matches(tag).trim_end();
        let new = if on { format!("{base}{tag}") } else { base.to_string() };
        self.status_text = new;

        // Persist to config.
        if let Some(base) = dirs::config_dir() {
            let path = base.join("telos").join("config.toml");
            let _ = save_auto_mode(&path, on);
        }
    }

    /// Send a user prompt to the background agent task.
    pub async fn send_prompt(&mut self, prompt: String) {
        self.cancellation.reset();
        crate::memory_runtime::record_user_preference(&self.memory, &prompt).await;
        self.chat.push_cell(Box::new(UserCell { content: prompt.clone() }));
        self.tool_activity.clear();
        self.base_status = self.status_text.clone();
        let _ = self.turn_tx.send(BackgroundCommand::Prompt(prompt));
        self.mode = Mode::Streaming;
        self.turn_active = true;
    }

    async fn handle_input_event(&mut self, event: InputEvent) {
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
        }
    }

    /// Process a popped overlay — extract results from selection popups, etc.
    async fn handle_overlay_popped(&mut self, popped: Option<Box<dyn Overlay>>) {
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

    fn refresh_mode_after_overlay(&mut self) {
        self.mode = if self.overlays.is_empty() {
            if self.turn_active { Mode::Streaming } else { Mode::Normal }
        } else {
            Mode::Approving
        };
    }

    fn open_approval_edit_popup(&mut self, pending: PendingApproval) {
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
            "deepseek-v4-pro".to_string(),
            "deepseek-v4-flash".to_string(),
        );
        let provider = Arc::new(telos_agent::RoutedProvider::new(config));
        let _ = self.turn_tx.send(BackgroundCommand::SetProvider {
            provider,
            label: "deepseek-v4-pro/deepseek-v4-flash".to_string(),
        });
    }

    fn switch_model(&mut self, model: &str) {
        let Some(api_key) = self.model_switch.deepseek_api_key.clone() else {
            self.chat.push_cell(Box::new(ErrorCell {
                message: "cannot switch model: missing DeepSeek API key".to_string(),
            }));
            return;
        };
        let provider = Arc::new(telos_agent::DeepSeekProvider::new(
            telos_agent::DeepSeekConfig::new(api_key, model.to_string()),
        ));
        let _ = self
            .turn_tx
            .send(BackgroundCommand::SetProvider { provider, label: model.to_string() });
        self.status_text = format!("telos · model {model}");
        self.base_status = self.status_text.clone();
        self.chat.push_cell(Box::new(UserCell { content: format!("/model {model}") }));
        self.chat.push_cell(Box::new(AgentCell {
            buffer: format!("Switched model to: {model}"),
            is_streaming: false,
        }));
    }

    async fn handle_session_action(&mut self, idx: usize) {
        match idx {
            0 => self.new_session(),
            1 => self.open_session_resume_popup(),
            2 => self.show_session_list(),
            _ => {}
        }
    }

    fn new_session(&mut self) {
        if self.turn_active {
            self.chat.push_cell(Box::new(ErrorCell {
                message: "wait for the current turn before starting a new session".to_string(),
            }));
            return;
        }
        self.chat.clear();
        self.tool_activity.clear();
        self.turn_input_tokens = 0;
        self.turn_output_tokens = 0;
        self.turn_started = None;
        self.cancellation.reset();
        let _ = self.turn_tx.send(BackgroundCommand::NewSession);
        self.status_text = "telos · new session".to_string();
        self.base_status = self.status_text.clone();
    }

    fn open_session_resume_popup(&mut self) {
        let sessions = self.session_ids();
        if sessions.is_empty() {
            self.chat.push_cell(Box::new(AgentCell {
                buffer: "No saved sessions found.".to_string(),
                is_streaming: false,
            }));
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
        self.chat.push_cell(Box::new(AgentCell { buffer: body, is_streaming: false }));
    }

    async fn resume_session(&mut self, session_id: &str) {
        if self.turn_active {
            self.chat.push_cell(Box::new(ErrorCell {
                message: "wait for the current turn before resuming a session".to_string(),
            }));
            return;
        }
        match self.storage.load(session_id).await {
            Ok(messages) => {
                self.chat.clear();
                self.tool_activity.clear();
                self.chat.push_cell(Box::new(AgentCell {
                    buffer: format!("Resumed session: {session_id}"),
                    is_streaming: false,
                }));
                for message in messages {
                    self.push_message_cell(message);
                }
                self.cancellation.reset();
                let _ = self.turn_tx.send(BackgroundCommand::ResumeSession(session_id.to_string()));
                self.status_text = format!("telos · session {session_id}");
                self.base_status = self.status_text.clone();
            }
            Err(err) => self.chat.push_cell(Box::new(ErrorCell {
                message: format!("failed to load session {session_id}: {err}"),
            })),
        }
    }

    fn push_message_cell(&mut self, message: telos_agent::Message) {
        match message.role {
            Role::System => {}
            Role::User => {
                let text = message.text_content();
                if !text.is_empty() {
                    self.chat.push_cell(Box::new(UserCell { content: text }));
                }
            }
            Role::Assistant => {
                let thinking = message.thinking_content();
                if !thinking.is_empty() {
                    self.chat.push_cell(Box::new(ThinkingCell {
                        buffer: thinking,
                        is_streaming: false,
                    }));
                }
                let text = message.text_content();
                if !text.is_empty() {
                    self.chat.push_cell(Box::new(AgentCell { buffer: text, is_streaming: false }));
                }
            }
            Role::Tool => {
                for result in message.tool_results_iter() {
                    let mut cell = ToolCallCell::new(
                        result.tool_call_id.clone(),
                        result.name.clone(),
                        result.content.to_string(),
                    );
                    cell.set_completed(!result.is_error);
                    self.chat.push_cell(Box::new(cell));
                }
            }
        }
    }

    fn session_ids(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.sessions_dir) else { return Vec::new() };
        let mut sessions = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
                    .then(|| path.file_stem()?.to_str().map(str::to_string))?
            })
            .collect::<Vec<_>>();
        sessions.sort();
        sessions.reverse();
        sessions
    }

    fn show_tool_summary(&mut self) {
        if self.tool_infos.is_empty() {
            self.chat.push_cell(Box::new(AgentCell {
                buffer: "No tools are registered.".to_string(),
                is_streaming: false,
            }));
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
        self.chat.push_cell(Box::new(AgentCell { buffer: lines.join("\n"), is_streaming: false }));
    }

    async fn handle_turn_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::TurnStarted { .. } => {
                self.status_text = "thinking…".to_string();
                self.turn_started = Some(Instant::now());
                self.turn_input_tokens = 0;
                self.turn_output_tokens = 0;
                self.turn_tool_calls = 0;
                self.turn_tool_failures = 0;
            }
            TurnEvent::AssistantDelta { text } => {
                self.status_text = "streaming…".to_string();
                self.chat.push_agent_delta(&text);
            }
            TurnEvent::ThinkingDelta { text } => {
                self.chat.push_thinking_delta(&text);
            }
            TurnEvent::ToolCall { tool_call_id, name, detail } => {
                let label = if detail.is_empty() { name.clone() } else { detail.clone() };
                self.status_text = label;
                self.turn_tool_calls = self.turn_tool_calls.saturating_add(1);
                self.tool_activity.push_call(tool_call_id, name, detail);
            }
            TurnEvent::ToolProgress { tool_call_id, message, .. } => {
                if !message.starts_with("running command with") {
                    self.status_text = message.to_string();
                }
                if let Some(ref id) = tool_call_id {
                    self.tool_activity.set_progress(id, message);
                }
            }
            TurnEvent::ToolCompleted { tool_call_id, name, is_error } => {
                let detail = self.tool_activity.complete(&tool_call_id, name.clone(), !is_error);
                if is_error {
                    self.turn_tool_failures = self.turn_tool_failures.saturating_add(1);
                }

                if !is_error {
                    crate::memory_runtime::record_successful_tool(
                        &self.memory,
                        &name,
                        &tool_call_id,
                        Some(&detail),
                    )
                    .await;
                }
            }
            TurnEvent::ToolResult(message) => {
                for result in message.tool_results_iter() {
                    self.tool_activity.add_result_content(
                        &result.tool_call_id,
                        &result.content,
                        result.is_error,
                    );
                    if result.is_error {
                        crate::memory_runtime::record_tool_error(&self.memory, result, None).await;
                    }
                }
            }
            TurnEvent::TurnFinished { final_text, .. } => {
                let had_streamed_assistant = self.chat.has_active_assistant();
                self.chat.finish_streaming_cells();
                if !final_text.is_empty() && !had_streamed_assistant {
                    self.chat
                        .push_cell(Box::new(AgentCell { buffer: final_text, is_streaming: false }));
                }
            }
            TurnEvent::TokenBudgetExceeded { used_tokens, max_tokens } => {
                self.chat.push_cell(Box::new(ErrorCell {
                    message: format!("token budget exceeded: {used_tokens}/{max_tokens}"),
                }));
            }
            TurnEvent::ProviderRetry { attempt, max_retries, delay_ms } => {
                self.status_text = format!("retrying ({attempt}/{max_retries}, {delay_ms}ms)");
            }
            TurnEvent::ProviderUsage { input_tokens, output_tokens } => {
                self.turn_input_tokens = input_tokens as u64;
                self.turn_output_tokens = output_tokens as u64;
            }
            _ => {}
        }
    }

    /// Draw the entire UI.
    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let theme = Theme::default();

        // Layout: chat | compact tool activity | input | status
        let activity_height = self.tool_activity.height(area.width as usize);
        let constraints = vec![
            Constraint::Min(0),                  // chat
            Constraint::Length(activity_height), // recent tool/command activity
            Constraint::Length(5),               // input panel
            Constraint::Length(1),               // status bar
        ];

        let layout =
            Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);

        self.chat.render(frame, layout[0], &theme);
        self.tool_activity.render(frame, layout[1], &theme);
        self.input.render(frame, layout[2], self.mode == Mode::Normal);

        // ── Status bar at the bottom ─────────────────────────────────
        let status = if self.turn_active {
            let elapsed = self.format_elapsed();
            let tokens = self.format_token_usage();
            if tokens.is_empty() {
                format!("{} ({})", self.status_text, elapsed)
            } else {
                format!("{} ({} | {})", self.status_text, elapsed, tokens)
            }
        } else {
            self.status_text.clone()
        };

        status_bar::render(
            frame,
            layout[3],
            &status,
            self.spinner_frame,
            self.turn_input_tokens + self.turn_output_tokens,
            self.token_budget_max,
        );

        // ── Render active overlay on top ─────────────────────────────
        if let Some(overlay) = self.overlays.last() {
            overlay.render(frame, area, &theme);
        }
    }
}

fn save_auto_mode(path: &std::path::Path, on: bool) -> anyhow::Result<()> {
    let contents = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };
    let mut config: toml::Value = if contents.is_empty() {
        toml::Value::Table(toml::Table::new())
    } else {
        toml::from_str(&contents).unwrap_or(toml::Value::Table(toml::Table::new()))
    };
    // Set auto_mode at the top level.
    config.as_table_mut().and_then(|t| t.insert("auto_mode".into(), toml::Value::Boolean(on)));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(&config)?)?;
    Ok(())
}

fn format_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    if ms < 60_000 {
        return format!("{:.1}s", ms as f64 / 1000.0);
    }
    let secs = ms / 1000;
    format!("{}m{}s", secs / 60, secs % 60)
}

fn format_turn_tokens(input: u64, output: u64) -> String {
    let total = input + output;
    if total == 0 {
        return "tokens n/a".to_string();
    }
    format!(
        "tokens ↑{} ↓{} total {}",
        format_token_count(input),
        format_token_count(output),
        format_token_count(total)
    )
}

fn format_token_count(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    format!("{:.1}k", tokens as f64 / 1000.0)
}

fn save_deepseek_api_key(path: &std::path::Path, key: &str) -> anyhow::Result<()> {
    let contents = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };
    let mut config: toml::Value = if contents.is_empty() {
        toml::Value::Table(toml::Table::new())
    } else {
        toml::from_str(&contents).unwrap_or(toml::Value::Table(toml::Table::new()))
    };

    let table =
        config.as_table_mut().ok_or_else(|| anyhow::anyhow!("config root is not a table"))?;
    let env = table.entry("env").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let env_table =
        env.as_table_mut().ok_or_else(|| anyhow::anyhow!("config [env] is not a table"))?;
    env_table.insert("DEEPSEEK_API_KEY".into(), toml::Value::String(key.to_string()));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(&config)?)?;
    Ok(())
}

fn collect_tool_infos(tools: &telos_agent::ToolRegistry) -> Vec<ToolInfo> {
    let mut infos = tools
        .definitions()
        .into_iter()
        .map(|definition| {
            let aliases = tools
                .get(&definition.name)
                .map(|tool| tool.aliases().iter().map(|alias| (*alias).to_string()).collect())
                .unwrap_or_else(|_| Vec::new());
            ToolInfo { name: definition.name, aliases, description: definition.description }
        })
        .collect::<Vec<_>>();
    infos.sort_by(|a, b| a.name.cmp(&b.name));
    infos
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[tokio::test]
    async fn send_prompt_resets_cancel_flag() {
        let cancelled = Arc::new(AtomicBool::new(true));
        let cancellation = telos_agent::CancellationState::from_flag(Arc::clone(&cancelled));
        let config =
            telos_agent::AgentConfig { cancellation, ..telos_agent::AgentConfig::default() };
        let provider = Arc::new(telos_agent::MockProvider::new(vec![]));
        let tools = telos_agent::ToolRegistry::new();
        let temp = tempfile::tempdir().unwrap();
        let memory = Arc::new(Mutex::new(MemoryStore::new(temp.path().join("memory"))));

        let mut app = App::new(
            config,
            provider,
            tools,
            "telos".into(),
            Some(temp.path()),
            false,
            memory,
            ModelSwitchConfig::default(),
        )
        .unwrap();

        app.send_prompt("hello".into()).await;

        assert!(!cancelled.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn normal_enter_does_not_toggle_activity() {
        let config = telos_agent::AgentConfig::default();
        let provider = Arc::new(telos_agent::MockProvider::new(vec![]));
        let tools = telos_agent::ToolRegistry::new();
        let temp = tempfile::tempdir().unwrap();
        let memory = Arc::new(Mutex::new(MemoryStore::new(temp.path().join("memory"))));

        let mut app = App::new(
            config,
            provider,
            tools,
            "telos".into(),
            Some(temp.path()),
            false,
            memory,
            ModelSwitchConfig::default(),
        )
        .unwrap();
        app.tool_activity.push_call("call-1".into(), "Bash".into(), "cargo test".into());
        app.tool_activity.complete("call-1", "Bash".into(), true);
        app.tool_activity.add_result_content(
            "call-1",
            &serde_json::json!({"stdout": "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n", "stderr": ""}),
            false,
        );

        let before = app.tool_activity.height(80);
        app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        )))
        .await
        .unwrap();

        assert_eq!(app.tool_activity.height(80), before);
    }

    #[test]
    fn turn_summary_formats_duration_tools_and_tokens() {
        assert_eq!(format_duration_ms(850), "850ms");
        assert_eq!(format_duration_ms(12_340), "12.3s");
        assert_eq!(format_duration_ms(65_000), "1m5s");
        assert_eq!(format_turn_tokens(12_300, 1_800), "tokens ↑12.3k ↓1.8k total 14.1k");
        assert_eq!(format_turn_tokens(0, 0), "tokens n/a");
    }
}
