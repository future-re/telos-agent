use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::pin::pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use telos_agent::{MemoryStore, TurnEvent};
use tokio::sync::mpsc;

use crate::tui::approval::PendingApproval;
use crate::tui::chat_panel::ChatPanel;
use crate::tui::event::Event;
use crate::tui::input_panel::InputPanel;
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

/// A UI-facing message fragment.
#[derive(Debug, Clone)]
pub enum UiMessage {
    /// Full user prompt.
    User(String),
    /// Incremental assistant text fragment.
    AssistantDelta(String),
    /// Incremental reasoning/thinking fragment.
    ThinkingDelta(String),
    /// Tool call started.
    ToolCall { id: String, name: String, detail: String },
    /// Tool progress update (detail shown inline under the tool).
    ToolProgress { id: Option<String>, name: String, message: String },
    /// Tool call finished.
    ToolCompleted { id: String, name: String, detail: String, is_error: bool },
    /// Marks the end of a turn.
    TurnComplete,
    /// An error message (turn stream failure, session error, etc.).
    Error(String),
}

/// Root application state for the TUI.
pub struct App {
    /// Current UI mode.
    pub mode: Mode,
    /// Whether the application should exit.
    pub should_quit: bool,
    /// Status text shown in the top bar.
    pub status_text: String,
    /// Accumulated messages for display.
    pub messages: Vec<UiMessage>,
    /// Chat panel (rendering + scrolling).
    pub chat: ChatPanel,
    /// Input panel at the bottom.
    pub input: InputPanel,
    /// Approval requests waiting for user decision.
    pub pending_approvals: VecDeque<PendingApproval>,
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
    /// Shared memory store for tools, prompt injection, and automatic feedback.
    memory: Arc<Mutex<MemoryStore>>,
    /// Tool call details keyed by tool_call_id for automatic memory entries.
    tool_details: HashMap<String, String>,
    /// Send prompts to the background agent task.
    turn_tx: mpsc::UnboundedSender<String>,
    /// Receive TurnEvents from the background agent task.
    turn_rx: mpsc::UnboundedReceiver<Event>,
    /// Receive pending approvals from the TuiApprovalHandler.
    approval_rx: mpsc::UnboundedReceiver<PendingApproval>,
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

        // Auto-approve mode — shared between UI and approval handler.
        let auto_mode = Arc::new(AtomicBool::new(auto_mode_on));
        let auto_mode_bg = Arc::clone(&auto_mode);

        // Seed status text with auto tag if starting in auto mode.
        let status_text =
            if auto_mode_on { format!("{status_text} ⏵⏵ auto") } else { status_text };

        let (prompt_tx, mut prompt_rx) = mpsc::unbounded_channel::<String>();
        let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();
        let (approval_tx, approval_rx) = mpsc::unbounded_channel::<PendingApproval>();

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
            messages: Vec::new(),
            chat: ChatPanel::new(),
            input: InputPanel::new(),
            pending_approvals: VecDeque::new(),
            turn_active: false,
            cancel_flag,
            auto_mode,
            turn_started: None,
            turn_input_tokens: 0,
            turn_output_tokens: 0,
            memory,
            tool_details: HashMap::new(),
            turn_tx: prompt_tx,
            turn_rx: event_rx,
            approval_rx,
        })
    }

    /// Process a single event.
    pub fn handle_event(&mut self, event: Event) -> anyhow::Result<()> {
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
                        self.messages.clear();
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
                        self.messages.clear();
                        self.chat.scroll_to_bottom();
                        self.status_text = "telos · new session (next prompt)".to_string();
                        return Ok(());
                    }
                    _ => {}
                }

                match self.mode {
                    Mode::Approving => {
                        match key.code {
                            KeyCode::Char('a') | KeyCode::Char('y') => self.approve_current(),
                            KeyCode::Char('d') | KeyCode::Char('n') => {
                                self.deny_current("denied by user");
                            }
                            KeyCode::Char('e') => {
                                // Future: open editor to modify arguments.
                                self.deny_current("edit requested");
                            }
                            _ => {}
                        }
                        return Ok(());
                    }
                    Mode::Normal => {
                        // Scroll keys: plain arrow keys scroll chat.
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
                            // Ctrl+Up/Down → input history (handled below).
                            _ => {}
                        }

                        // Input handling (Ctrl+Up/Down → history, other keys → typing).
                        if let Some(prompt) = self.input.handle_key(key) {
                            self.send_prompt(prompt);
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
                while let Ok(event) = self.turn_rx.try_recv() {
                    self.handle_event(event)?;
                }
                while let Ok(pending) = self.approval_rx.try_recv() {
                    self.pending_approvals.push_back(pending);
                    self.mode = Mode::Approving;
                }
            }
            Event::SessionError { message } => {
                self.messages.push(UiMessage::Error(message));
                self.mode = Mode::Normal;
                self.turn_active = false;
            }
            Event::Resize { .. } => {}
            Event::Turn(turn_event) => self.handle_turn_event(turn_event),
            Event::TurnComplete => {
                self.messages.push(UiMessage::TurnComplete);
                self.mode = Mode::Normal;
                self.turn_active = false;
                self.turn_started = None;
                self.turn_input_tokens = 0;
                self.turn_output_tokens = 0;
                self.status_text = self.base_status.clone();
            }
            Event::Mouse(_) => {}
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
    pub fn send_prompt(&mut self, prompt: String) {
        crate::memory_runtime::record_user_preference(&self.memory, &prompt);
        self.messages.push(UiMessage::User(prompt.clone()));
        self.base_status = self.status_text.clone();
        let _ = self.turn_tx.send(prompt);
        self.mode = Mode::Streaming;
        self.turn_active = true;
    }

    /// Approve the current pending approval request.
    pub fn approve_current(&mut self) {
        if let Some(pending) = self.pending_approvals.pop_front() {
            let _ = pending.respond.send(telos_agent::ApprovalDecision::Allow);
        }
        if self.pending_approvals.is_empty() {
            self.mode = if self.turn_active { Mode::Streaming } else { Mode::Normal };
        }
    }

    /// Deny the current pending approval request with a reason.
    pub fn deny_current(&mut self, reason: &str) {
        if let Some(pending) = self.pending_approvals.pop_front() {
            let _ = pending
                .respond
                .send(telos_agent::ApprovalDecision::Deny { reason: reason.to_string() });
        }
        if self.pending_approvals.is_empty() {
            self.mode = if self.turn_active { Mode::Streaming } else { Mode::Normal };
        }
    }

    /// Convert an agent `TurnEvent` into a `UiMessage`.
    fn handle_turn_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::TurnStarted { .. } => {
                self.status_text = "thinking…".to_string();
                self.turn_started = Some(Instant::now());
                self.turn_input_tokens = 0;
                self.turn_output_tokens = 0;
            }
            TurnEvent::AssistantDelta { text } => {
                self.status_text = "streaming…".to_string();
                self.messages.push(UiMessage::AssistantDelta(text));
                self.chat.scroll_to_bottom();
            }
            TurnEvent::ThinkingDelta { text } => {
                self.messages.push(UiMessage::ThinkingDelta(text));
            }
            TurnEvent::ToolCall { tool_call_id, name, detail } => {
                self.tool_details.insert(tool_call_id.clone(), detail.clone());
                let label = if detail.is_empty() { name.clone() } else { detail.clone() };
                self.status_text = label;
                self.messages.push(UiMessage::ToolCall { id: tool_call_id, name, detail });
            }
            TurnEvent::ToolProgress { tool_call_id, name, message, .. } => {
                if !message.starts_with("running command with") {
                    self.status_text = format!("{} | {}", name, message);
                }
                self.messages.push(UiMessage::ToolProgress { id: tool_call_id, name, message });
            }
            TurnEvent::ToolCompleted { tool_call_id, name, is_error } => {
                // Grab the detail from the pending ToolCall before removing it.
                let detail = self
                    .messages
                    .iter()
                    .find_map(|m| match m {
                        UiMessage::ToolCall { id, detail, .. } if id == &tool_call_id => {
                            Some(detail.clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_default();
                // Remove pending + progress, replace with completed.
                // Also remove any id-less ToolProgress (they're stale once the tool completes).
                self.messages.retain(|m| match m {
                    UiMessage::ToolCall { id, .. } => id != &tool_call_id,
                    UiMessage::ToolProgress { id, .. } => {
                        // Remove if the id matches, OR if id is None (stale).
                        id.is_some() && id.as_deref() != Some(&tool_call_id)
                    }
                    _ => true,
                });
                self.messages.push(UiMessage::ToolCompleted {
                    id: tool_call_id.clone(),
                    name: name.clone(),
                    detail: detail.clone(),
                    is_error,
                });

                if !is_error {
                    crate::memory_runtime::record_successful_tool(
                        &self.memory,
                        &name,
                        &tool_call_id,
                        self.tool_details.get(&tool_call_id).map(String::as_str),
                    );
                }
            }
            TurnEvent::ToolResult(message) => {
                for result in message.tool_results_iter() {
                    if result.is_error {
                        crate::memory_runtime::record_tool_error(
                            &self.memory,
                            result,
                            self.tool_details.get(&result.tool_call_id).map(String::as_str),
                        );
                    }
                }
            }
            TurnEvent::TurnFinished { final_text, .. } => {
                if !final_text.is_empty() {
                    self.messages.push(UiMessage::AssistantDelta(final_text));
                }
            }
            TurnEvent::TokenBudgetExceeded { used_tokens, max_tokens } => {
                self.messages.push(UiMessage::Error(format!(
                    "token budget exceeded: {used_tokens}/{max_tokens}"
                )));
            }
            TurnEvent::ProviderRetry { attempt, max_retries, delay_ms } => {
                let elapsed = self.format_elapsed();
                self.status_text =
                    format!("retrying ({attempt}/{max_retries}, {delay_ms}ms){elapsed}");
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

        // ── Build constraints dynamically ─────────────────────────────
        let popup_h = if self.mode == Mode::Approving
            && let Some(pending) = self.pending_approvals.front()
        {
            let max_w = area.width.saturating_sub(10);
            let inner_w = (max_w.saturating_sub(2)).max(40) as usize;
            // Count content lines for height.
            let content_lines = approval_content_lines(
                &pending.request.tool_name,
                &pending.request.arguments,
                inner_w,
            );
            // Title(1) + content + hints(1) + border(2)
            let h = 1 + content_lines + 1 + 2;
            let max_h = ((area.height as f32) * 0.5) as u16;
            Some(h.min(max_h as usize).max(8) as u16)
        } else {
            None
        };

        // Layout: chat | popup? | input | status
        let mut constraints: Vec<Constraint> = vec![Constraint::Min(0)]; // chat
        if let Some(h) = popup_h {
            constraints.push(Constraint::Length(h + 1)); // popup + padding
        }
        constraints.push(Constraint::Length(5)); // input panel
        constraints.push(Constraint::Length(1)); // status bar (bottom)

        let layout =
            Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);

        let mut idx = 0;

        self.chat.render(frame, layout[idx], &self.messages);
        idx += 1;

        // ── Render approval popup in its own layout slot ──────────────
        if let Some(_h) = popup_h
            && let Some(pending) = self.pending_approvals.front()
        {
            let popup_area = layout[idx];
            idx += 1;

            let args = &pending.request.arguments;
            let tool_name = &pending.request.tool_name;

            let block = Block::default()
                .title(" Approval required ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.tool_pending_fg))
                .style(Style::default().bg(Color::Rgb(20, 22, 30)));

            let mut text_lines: Vec<Line> = Vec::new();

            // Tool name line.
            text_lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    tool_name.clone(),
                    Style::default().fg(theme.tool_pending_fg).add_modifier(Modifier::BOLD),
                ),
            ]));

            // ── Tool-specific content ────────────────────────────
            let tool_lower = tool_name.to_lowercase();
            if tool_lower == "bash" || tool_lower == "shell" {
                if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                    text_lines.push(Line::from(""));
                    text_lines.push(Line::from(Span::styled(
                        format!("  $ {}", truncate_for_popup(cmd, 200)),
                        Style::default().fg(Color::Rgb(180, 220, 180)),
                    )));
                }
            } else if tool_lower == "edit" {
                let file = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
                let old = args.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
                let new = args.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
                text_lines.push(Line::from(Span::styled(
                    format!("  File: {}", truncate_for_popup(file, 120)),
                    Style::default().fg(Color::Gray),
                )));
                text_lines.push(Line::from(""));
                text_lines.push(Line::from(Span::styled(
                    format!("  - {}", truncate_for_popup(old, 150)),
                    Style::default().fg(Color::Rgb(220, 120, 120)),
                )));
                text_lines.push(Line::from(Span::styled(
                    format!("  + {}", truncate_for_popup(new, 150)),
                    Style::default().fg(Color::Rgb(120, 220, 120)),
                )));
            } else if tool_lower == "write" {
                let file = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                text_lines.push(Line::from(Span::styled(
                    format!("  File: {}", truncate_for_popup(file, 120)),
                    Style::default().fg(Color::Gray),
                )));
                let preview = truncate_for_popup(content, 300);
                if !preview.is_empty() {
                    text_lines.push(Line::from(""));
                    for pline in preview.lines().take(6) {
                        text_lines.push(Line::from(Span::styled(
                            format!("  | {}", pline),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                }
            } else {
                // Generic: show pretty JSON.
                let args_str =
                    serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
                text_lines.push(Line::from(""));
                for aline in args_str.lines().take(20) {
                    text_lines.push(Line::from(Span::styled(
                        format!("  {}", aline),
                        Style::default().fg(Color::Gray),
                    )));
                }
            }

            text_lines.push(Line::from(""));
            text_lines.push(Line::from(Span::styled(
                "  [a/y] approve  [d/n] deny  [e] edit-request  ",
                Style::default().fg(Color::White),
            )));

            let text = Text::from(text_lines);
            let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: true });

            frame.render_widget(Clear, popup_area);
            frame.render_widget(paragraph, popup_area);
        }

        self.input.render(frame, layout[idx], self.mode == Mode::Normal);
        idx += 1;

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

        status_bar::render(frame, layout[idx], &status);
    }
}

/// Count how many lines `text` will occupy when wrapped at `width` columns.
fn count_wrapped_lines(text: &str, width: usize) -> usize {
    text.lines()
        .map(|line| {
            let chars = line.chars().count();
            if chars == 0 { 1 } else { (chars + width.saturating_sub(1)) / width }
        })
        .sum::<usize>()
        .max(1)
}

fn approval_content_lines(tool_name: &str, args: &serde_json::Value, width: usize) -> usize {
    let tool_lower = tool_name.to_lowercase();
    let mut lines = 1usize; // tool name line

    if tool_lower == "bash" || tool_lower == "shell" {
        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            lines += 1; // blank
            lines += count_wrapped_lines(&format!("  $ {}", truncate_for_popup(cmd, 200)), width);
        }
    } else if tool_lower == "edit" {
        let file = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        let old = args.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
        let new = args.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
        lines += count_wrapped_lines(&format!("  File: {}", truncate_for_popup(file, 120)), width);
        lines += 1;
        lines += count_wrapped_lines(&format!("  - {}", truncate_for_popup(old, 150)), width);
        lines += count_wrapped_lines(&format!("  + {}", truncate_for_popup(new, 150)), width);
    } else if tool_lower == "write" {
        let file = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
        lines += count_wrapped_lines(&format!("  File: {}", truncate_for_popup(file, 120)), width);
        let preview = truncate_for_popup(content, 300);
        if !preview.is_empty() {
            lines += 1;
            for pline in preview.lines().take(6) {
                lines += count_wrapped_lines(&format!("  | {}", pline), width);
            }
        }
    } else {
        let args_str = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
        lines += 1;
        for aline in args_str.lines().take(20) {
            lines += count_wrapped_lines(&format!("  {}", aline), width);
        }
    }
    lines
}

fn truncate_for_popup(s: &str, max_chars: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let keep = max_chars.saturating_sub(1);
        format!("{}…", s.chars().take(keep).collect::<String>())
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

#[cfg(test)]
mod tests {
    use super::truncate_for_popup;

    #[test]
    fn truncate_for_popup_handles_utf8_boundaries() {
        let truncated = truncate_for_popup("中文命令🙂测试", 5);
        assert_eq!(truncated, "中文命令…");
    }

    #[test]
    fn truncate_for_popup_leaves_short_text_unchanged() {
        assert_eq!(truncate_for_popup("hello", 10), "hello");
    }
}
