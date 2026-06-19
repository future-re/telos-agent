use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use std::pin::pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use telos_agent::{MemoryStore, TurnEvent};
use tokio::sync::mpsc;

use crate::tui::approval::PendingApproval;
use crate::tui::chat_widget::ChatWidget;
use crate::tui::command_popup::SlashCommand;
use crate::tui::event::{AppEvent, Event};
use crate::tui::history_cell::*;
use crate::tui::input_panel::{InputEvent, InputPanel};
use crate::tui::overlay::{ApprovalOverlay, Overlay, OverlayAction};
use crate::tui::selection_popup::SelectionPopup;
use crate::tui::status_bar;
use crate::tui::theme::Theme;

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
    /// Active overlay stack (topmost overlay rendered last).
    pub overlays: Vec<Box<dyn Overlay>>,
    /// Whether a background turn is currently running.
    pub turn_active: bool,
    /// Saved base status text — restored after each turn.
    base_status: String,
    /// Shared cancellation flag — set by Ctrl+C and read by the background task.
    cancel_flag: Arc<AtomicBool>,
    /// Auto-approve mode — toggle with Shift+Tab.
    auto_mode: Arc<AtomicBool>,
    /// When the current turn started (for elapsed display).
    turn_started: Option<Instant>,
    /// Tokens consumed in the current turn.
    turn_input_tokens: u64,
    turn_output_tokens: u64,
    /// Spinner animation frame (incremented on Tick).
    spinner_frame: usize,
    /// Maximum tokens for the budget progress bar.
    token_budget_max: Option<u64>,
    /// Shared memory store for tools, prompt injection, and automatic feedback.
    memory: Arc<Mutex<MemoryStore>>,
    /// Send prompts to the background agent task.
    turn_tx: mpsc::UnboundedSender<String>,
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
    ) -> Result<Self, telos_agent::AgentError> {
        // Wire up session storage before creating the AgentSession.
        let session_manager = crate::session::SessionManager::new(project_root);
        std::fs::create_dir_all(session_manager.sessions_dir()).ok();
        let storage =
            Arc::new(telos_agent::JsonlStorage::new(session_manager.sessions_dir().to_path_buf())?);
        config.storage = Some(storage);

        // Extract the cancellation flag before moving config into the spawned task.
        let cancel_flag = Arc::clone(&config.cancelled);
        let token_budget_max = config.token_budget.as_ref().map(|b| b.max_tokens as u64);

        // Auto-approve mode — shared between UI and approval handler.
        let auto_mode = Arc::new(AtomicBool::new(auto_mode_on));
        let auto_mode_bg = Arc::clone(&auto_mode);

        // Seed status text with auto tag if starting in auto mode.
        let status_text =
            if auto_mode_on { format!("{status_text} ⏵⏵ auto") } else { status_text };

        let (prompt_tx, mut prompt_rx) = mpsc::unbounded_channel::<String>();
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

            let mut session = match telos_agent::AgentSession::new(config) {
                Ok(s) => s,
                Err(e) => {
                    // Surface the error so the TUI can display it.
                    let _ = event_tx.send(Event::SessionError { message: e.to_string() });
                    let _ = event_tx.send(Event::TurnComplete);
                    return;
                }
            };

            while let Some(prompt) = prompt_rx.recv().await {
                let erased = telos_agent::ErasedProvider(provider.as_ref());
                {
                    let mut stream = pin!(session.run_turn_stream(&erased, &tools, prompt,));
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(te) => {
                                let _ = event_tx.send(Event::Turn(te));
                            }
                            Err(e) => {
                                let _ =
                                    event_tx.send(Event::SessionError { message: e.to_string() });
                                break;
                            }
                        }
                    }
                }
                let _ = session.save().await;
                let _ = event_tx.send(Event::TurnComplete);
            }
        });

        Ok(Self {
            mode: Mode::Normal,
            should_quit: false,
            status_text: status_text.clone(),
            base_status: status_text,
            chat: ChatWidget::new(),
            input: InputPanel::new(),
            overlays: Vec::new(),
            turn_active: false,
            cancel_flag,
            auto_mode,
            turn_started: None,
            turn_input_tokens: 0,
            turn_output_tokens: 0,
            memory,
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
                        self.cancel_flag.store(true, Ordering::Relaxed);
                        self.turn_active = false;
                        self.turn_started = None;
                        self.status_text = self.base_status.clone();
                        return Ok(());
                    }
                    (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                        self.chat.clear();
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
                        // Full session reset requires recreating the background AgentSession,
                        // which is a follow-up enhancement. For now, clear the chat and indicate
                        // that a new session will begin on the next prompt.
                        self.chat.clear();
                        self.chat.scroll_to_bottom();
                        self.status_text = "telos · new session (next prompt)".to_string();
                        return Ok(());
                    }
                    _ => {}
                }

                match self.mode {
                    Mode::Approving => {
                        if let Some(overlay) = self.overlays.last_mut()
                            && overlay.handle_key(key) == OverlayAction::Pop
                        {
                            let popped = self.overlays.pop();
                            self.handle_overlay_popped(popped);
                            self.mode = if self.overlays.is_empty() {
                                if self.turn_active { Mode::Streaming } else { Mode::Normal }
                            } else {
                                Mode::Approving
                            };
                        }
                        return Ok(());
                    }
                    Mode::Normal => {
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
                            _ => {}
                        }

                        // Input handling with InputEvent
                        match self.input.handle_key(key) {
                            InputEvent::Submit(prompt) => {
                                self.send_prompt(prompt).await;
                            }
                            InputEvent::SlashCommand(cmd) => {
                                self.handle_slash_command(cmd).await;
                            }
                            InputEvent::None => {}
                        }
                    }
                    Mode::Streaming => {
                        // During streaming, only scroll keys are handled.
                        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                        match (key.code, ctrl) {
                            (KeyCode::PageUp, _) => self.chat.scroll_up(10),
                            (KeyCode::PageDown, _) => self.chat.scroll_down(10),
                            (KeyCode::Up, false) => self.chat.scroll_up(1),
                            (KeyCode::Down, false) => self.chat.scroll_down(1),
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
                            self.chat.push_cell(Box::new(SeparatorCell));
                            self.mode = Mode::Normal;
                            self.turn_active = false;
                            self.turn_started = None;
                            self.turn_input_tokens = 0;
                            self.turn_output_tokens = 0;
                            self.status_text = self.base_status.clone();
                        }
                        Event::SessionError { message } => {
                            self.chat.push_cell(Box::new(ErrorCell { message }));
                            self.mode = Mode::Normal;
                            self.turn_active = false;
                            self.turn_started = None;
                            self.turn_input_tokens = 0;
                            self.turn_output_tokens = 0;
                            self.status_text = self.base_status.clone();
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
        crate::memory_runtime::record_user_preference(&self.memory, &prompt).await;
        self.chat.push_cell(Box::new(UserCell { content: prompt.clone() }));
        self.base_status = self.status_text.clone();
        let _ = self.turn_tx.send(prompt);
        self.mode = Mode::Streaming;
        self.turn_active = true;
    }

    async fn handle_slash_command(&mut self, cmd: SlashCommand) {
        match cmd {
            SlashCommand::Help => {
                let help_text = "\
Available commands:\n\n  /tool   — configure tools\n\
  /model  — switch model\n\
  /help   — show this help\n\
  /clear  — clear conversation\n\
  /session — session management\n\
  /auto   — toggle auto-approve mode";
                self.chat.push_cell(Box::new(UserCell { content: format!("/{cmd:?}") }));
                self.chat.push_cell(Box::new(AgentCell {
                    buffer: help_text.to_string(),
                    is_streaming: false,
                }));
            }
            SlashCommand::Clear => {
                // App already has Ctrl+L for clear; /clear does the same
                self.chat.clear();
                self.chat.scroll_to_bottom();
            }
            SlashCommand::Auto => {
                let on = !self.auto_mode.load(Ordering::Relaxed);
                self.auto_mode.store(on, Ordering::Relaxed);
                self.update_auto_mode_status();
            }
            SlashCommand::Model => {
                let models = vec!["deepseek-v4-flash", "deepseek-v4-pro"];
                let popup = SelectionPopup::new(" Select model ", models);
                self.overlays.push(Box::new(popup));
                self.mode = Mode::Approving;
            }
            SlashCommand::Tool => {
                self.chat.push_cell(Box::new(AgentCell {
                    buffer: "Tool configuration not yet available.".to_string(),
                    is_streaming: false,
                }));
            }
            SlashCommand::Session => {
                self.chat.push_cell(Box::new(AgentCell {
                    buffer: "Session management not yet available.".to_string(),
                    is_streaming: false,
                }));
            }
        }
    }

    /// Process a popped overlay — extract results from selection popups, etc.
    fn handle_overlay_popped(&mut self, popped: Option<Box<dyn Overlay>>) {
        let Some(overlay) = popped else { return };
        if let Some(popup) = overlay.as_any().downcast_ref::<SelectionPopup>()
            && let Some(idx) = popup.selected_index()
        {
            const MODELS: [&str; 2] = ["deepseek-v4-flash", "deepseek-v4-pro"];
            if let Some(model) = MODELS.get(idx) {
                self.chat.push_cell(Box::new(UserCell { content: format!("/model {model}") }));
                self.chat.push_cell(Box::new(AgentCell {
                    buffer: format!("Switched model to: {model}"),
                    is_streaming: false,
                }));
            }
        }
    }

    async fn handle_turn_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::TurnStarted { .. } => {
                self.status_text = "thinking…".to_string();
                self.turn_started = Some(Instant::now());
                self.turn_input_tokens = 0;
                self.turn_output_tokens = 0;
            }
            TurnEvent::AssistantDelta { text } => {
                self.status_text = "streaming…".to_string();
                if self.chat.active_mut().is_none_or(|c| !c.is_streaming()) {
                    // New agent turn — push a fresh streaming cell
                    self.chat.push_cell(Box::new(AgentCell {
                        buffer: text.clone(),
                        is_streaming: true,
                    }));
                } else {
                    self.chat.push_text(&text);
                }
                self.chat.scroll_to_bottom();
            }
            TurnEvent::ThinkingDelta { text } => {
                if self.chat.active_mut().is_none_or(|c| !c.is_streaming()) {
                    self.chat.push_cell(Box::new(ThinkingCell { buffer: text.clone() }));
                } else {
                    self.chat.push_text(&text);
                }
            }
            TurnEvent::ToolCall { tool_call_id, name, detail } => {
                let label = if detail.is_empty() { name.clone() } else { detail.clone() };
                self.status_text = label;
                self.chat.push_cell(Box::new(ToolCallCell::new(tool_call_id, name, detail)));
            }
            TurnEvent::ToolProgress { tool_call_id, message, .. } => {
                if !message.starts_with("running command with") {
                    self.status_text = message.to_string();
                }
                // Find the ToolCallCell and add progress
                if let Some(ref id) = tool_call_id
                    && let Some(cell) = self.chat.find_tool_call_mut(id)
                    && let Some(tc) = cell.as_any_mut().downcast_mut::<ToolCallCell>()
                {
                    tc.add_progress(message);
                }
            }
            TurnEvent::ToolCompleted { tool_call_id, name, is_error } => {
                // Replace the pending ToolCallCell with a completed one
                let detail = self
                    .chat
                    .find_tool_call(&tool_call_id)
                    .and_then(|c| c.as_any().downcast_ref::<ToolCallCell>())
                    .map(|tc| tc.detail.clone())
                    .unwrap_or_default();

                self.chat.remove_tool_call(&tool_call_id);
                let mut cell =
                    ToolCallCell::new(tool_call_id.clone(), name.clone(), detail.clone());
                cell.set_completed(!is_error);
                self.chat.push_cell(Box::new(cell));

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
                    if result.is_error {
                        crate::memory_runtime::record_tool_error(&self.memory, result, None).await;
                    }
                }
            }
            TurnEvent::TurnFinished { final_text, .. } => {
                if !final_text.is_empty() {
                    // Mark the streaming cell as done and add final text
                    if let Some(active) = self.chat.active_mut()
                        && active.is_streaming()
                    {
                        // AgentCell is no longer streaming
                    }
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
    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        let theme = Theme::default();

        // Layout: chat | input | status
        let constraints = vec![
            Constraint::Min(0),    // chat
            Constraint::Length(5), // input panel
            Constraint::Length(1), // status bar
        ];

        let layout =
            Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);

        self.chat.render(frame, layout[0], &theme);
        self.input.render(frame, layout[1], self.mode == Mode::Normal);

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
            layout[2],
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
