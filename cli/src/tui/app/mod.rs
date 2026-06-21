use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use telos_agent::{MemoryStore, Storage};
use tokio::sync::mpsc;

mod background;
mod commands;
mod config;
mod events;
mod session_list;
mod sessions;
mod tasks;
mod tools;
mod turn_events;
mod turn_summary;

use crate::billing::CostCalculator;
use crate::config::{BillingSection, TuiDensity};
use crate::tui::approval::PendingApproval;
use crate::tui::approval_inline;
use crate::tui::chat_entry::ChatEntry;
use crate::tui::chat_widget::ChatWidget;
use crate::tui::event::{AppEvent, Event};
use crate::tui::input_panel::InputPanel;
use crate::tui::overlay::Overlay;
use crate::tui::status_bar;
use crate::tui::theme::Theme;
use background::{BackgroundCommand, spawn_background_session};
use config::save_auto_mode;
use tasks::task_dir_for_root;
use tools::{ToolInfo, collect_tool_infos};
use turn_summary::{format_duration_ms, format_turn_tokens};

const MODEL_OPTIONS: [&str; 3] = ["hybrid", "pro", "flash"];

#[derive(Debug, Clone, Default)]
pub struct ModelSwitchConfig {
    pub deepseek_api_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiLayoutSettings {
    pub input_height: u16,
}

impl TuiLayoutSettings {
    pub fn from_density(density: TuiDensity) -> Self {
        match density {
            TuiDensity::Compact => Self { input_height: 4 },
            TuiDensity::Default => Self { input_height: 5 },
            TuiDensity::Spacious => Self { input_height: 8 },
        }
    }
}

impl Default for TuiLayoutSettings {
    fn default() -> Self {
        Self::from_density(TuiDensity::Default)
    }
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
    /// Density-derived vertical layout settings.
    layout_settings: TuiLayoutSettings,
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
    turn_total_tokens: Option<u64>,
    turn_prompt_cache_hit_tokens: Option<u64>,
    turn_prompt_cache_miss_tokens: Option<u64>,
    turn_reasoning_tokens: Option<u64>,
    turn_has_provider_usage: bool,
    /// Tool usage counters for the current turn.
    turn_tool_calls: u64,
    turn_tool_failures: u64,
    /// Spinner animation frame (incremented on Tick).
    spinner_frame: usize,
    /// Maximum tokens for the budget progress bar.
    token_budget_max: Option<u64>,
    /// Cost calculator built from the billing configuration.
    cost_calculator: CostCalculator,
    /// Estimated cost of the current turn in the configured currency.
    turn_cost: f64,
    /// Shared memory store for tools, prompt injection, and automatic feedback.
    memory: Arc<Mutex<MemoryStore>>,
    /// Session storage used by the background session and TUI resume UI.
    storage: Arc<dyn Storage>,
    /// On-disk directory containing JSONL sessions.
    sessions_dir: PathBuf,
    /// On-disk directory containing persisted task JSON files.
    task_dir: PathBuf,
    /// Model switch settings.
    model_switch: ModelSwitchConfig,
    /// Snapshot of registered tool metadata for /tool.
    tool_infos: Vec<ToolInfo>,
    /// Approval request currently being edited in a UserInputPopup.
    editing_approval: Option<PendingApproval>,
    /// Approval request currently shown in the inline approval panel.
    inline_approval: Option<PendingApproval>,
    /// Whether the active inline approval command detail is expanded.
    inline_approval_expanded: bool,
    /// Last area used to render the inline approval panel.
    inline_approval_area: Option<Rect>,
    /// Pending approval requests waiting for the inline panel.
    inline_approval_queue: VecDeque<PendingApproval>,
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
        config: telos_agent::AgentConfig,
        provider: Arc<dyn telos_agent::ModelProvider>,
        tools: telos_agent::ToolRegistry,
        status_text: String,
        project_root: Option<&std::path::Path>,
        project_root_or_cwd: &std::path::Path,
        auto_mode_on: bool,
        memory: Arc<Mutex<MemoryStore>>,
        model_switch: ModelSwitchConfig,
        billing: Option<BillingSection>,
    ) -> Result<Self, telos_agent::AgentError> {
        Self::new_with_layout_settings(
            config,
            provider,
            tools,
            status_text,
            project_root,
            project_root_or_cwd,
            auto_mode_on,
            memory,
            model_switch,
            TuiLayoutSettings::default(),
            billing,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_layout_settings(
        mut config: telos_agent::AgentConfig,
        provider: Arc<dyn telos_agent::ModelProvider>,
        tools: telos_agent::ToolRegistry,
        status_text: String,
        project_root: Option<&std::path::Path>,
        project_root_or_cwd: &std::path::Path,
        auto_mode_on: bool,
        memory: Arc<Mutex<MemoryStore>>,
        model_switch: ModelSwitchConfig,
        layout_settings: TuiLayoutSettings,
        billing: Option<BillingSection>,
    ) -> Result<Self, telos_agent::AgentError> {
        // Wire up session storage before creating the AgentSession.
        let session_manager = crate::session::SessionManager::new(project_root);
        std::fs::create_dir_all(session_manager.sessions_dir()).ok();
        let sessions_dir = session_manager.sessions_dir().to_path_buf();
        let task_dir = task_dir_for_root(project_root_or_cwd);
        let storage: Arc<dyn Storage> =
            Arc::new(telos_agent::JsonlStorage::new(session_manager.sessions_dir().to_path_buf())?);
        config.storage = Some(storage.clone());
        let app_storage = storage.clone();

        // Extract cancellation state before moving config into the spawned task.
        let cancellation = config.cancellation.clone();
        let token_budget_max = config.token_budget.as_ref().map(|b| b.max_tokens as u64);

        // Auto-approve mode — shared between UI and approval handler.
        let auto_mode = Arc::new(AtomicBool::new(auto_mode_on));
        let auto_mode_bg = Arc::clone(&auto_mode);

        // Seed status text with auto tag if starting in auto mode.
        let status_text = if auto_mode_on { format!("{status_text} · auto") } else { status_text };

        let tool_infos = collect_tool_infos(&tools);

        let (prompt_tx, prompt_rx) = mpsc::unbounded_channel::<BackgroundCommand>();
        let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();
        let (approval_tx, approval_rx) = mpsc::unbounded_channel::<PendingApproval>();
        let (app_event_tx, app_event_rx) = mpsc::unbounded_channel::<AppEvent>();

        spawn_background_session(
            config,
            provider,
            tools,
            storage,
            auto_mode_bg,
            approval_tx,
            event_tx,
            prompt_rx,
        );

        // Strip auto tag from base_status so update_auto_mode_status can re-add it cleanly.
        let base = status_text.trim_end_matches(" · auto").to_string();

        Ok(Self {
            mode: Mode::Normal,
            should_quit: false,
            status_text: status_text.clone(),
            base_status: base,
            chat: ChatWidget::new(),
            input: InputPanel::new(),
            layout_settings,
            overlays: Vec::new(),
            turn_active: false,
            cancellation,
            auto_mode,
            turn_started: None,
            turn_input_tokens: 0,
            turn_output_tokens: 0,
            turn_total_tokens: None,
            turn_prompt_cache_hit_tokens: None,
            turn_prompt_cache_miss_tokens: None,
            turn_reasoning_tokens: None,
            turn_has_provider_usage: false,
            turn_tool_calls: 0,
            turn_tool_failures: 0,
            memory,
            storage: app_storage,
            sessions_dir,
            task_dir,
            model_switch,
            tool_infos,
            editing_approval: None,
            inline_approval: None,
            inline_approval_expanded: false,
            inline_approval_area: None,
            inline_approval_queue: VecDeque::new(),
            spinner_frame: 0,
            token_budget_max,
            cost_calculator: CostCalculator::from_section(billing.as_ref()),
            turn_cost: 0.0,
            turn_tx: prompt_tx,
            turn_rx: event_rx,
            approval_rx,
            app_event_tx,
            app_event_rx,
        })
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
        let total =
            self.turn_total_tokens.unwrap_or(self.turn_input_tokens + self.turn_output_tokens);
        if total == 0 {
            return String::new();
        }
        let up_k = self.turn_input_tokens as f64 / 1000.0;
        let down_k = self.turn_output_tokens as f64 / 1000.0;
        let tokens = format!("↑{:.1}k ↓{:.1}k", up_k, down_k);
        if self.turn_cost > 0.0 {
            format!("{} · {}", tokens, CostCalculator::format_cost(self.turn_cost))
        } else {
            tokens
        }
    }

    fn input_panel_height(&self, width: usize) -> u16 {
        self.input.desired_height(
            width,
            self.layout_settings.input_height,
            self.layout_settings.input_height.saturating_add(4),
        )
    }

    fn reset_turn_usage(&mut self) {
        self.turn_input_tokens = 0;
        self.turn_output_tokens = 0;
        self.turn_total_tokens = None;
        self.turn_prompt_cache_hit_tokens = None;
        self.turn_prompt_cache_miss_tokens = None;
        self.turn_reasoning_tokens = None;
        self.turn_has_provider_usage = false;
        self.turn_cost = 0.0;
    }

    fn push_turn_summary(&mut self) {
        let Some(summary) = self.format_turn_summary() else { return };
        self.chat.push_entry(ChatEntry::turn_summary(summary));
    }

    fn has_visible_turn_activity(&self) -> bool {
        self.turn_active
            || self.turn_started.is_some()
            || self.turn_tool_calls > 0
            || self.turn_input_tokens > 0
            || self.turn_output_tokens > 0
            || self.chat.has_active_assistant()
    }

    fn finalize_turn_ui(&mut self) {
        self.chat.finish_streaming_cells();
        self.push_turn_summary();
        self.chat.push_entry(ChatEntry::separator());
    }

    fn reset_turn_state(&mut self) {
        // Keep Approving mode if an overlay is still open (e.g. /session
        // popup was opened during streaming).
        self.mode = if self.overlays.is_empty() { Mode::Normal } else { Mode::Approving };
        self.turn_active = false;
        self.turn_started = None;
        self.reset_turn_usage();
        self.turn_tool_calls = 0;
        self.turn_tool_failures = 0;
        self.status_text = self.base_status.clone();
        self.update_auto_mode_status();
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
        let token_text = format_turn_tokens(
            self.turn_input_tokens,
            self.turn_output_tokens,
            self.turn_total_tokens,
            self.turn_prompt_cache_hit_tokens,
            self.turn_prompt_cache_miss_tokens,
            self.turn_reasoning_tokens,
        );

        let cost_text = if self.turn_cost > 0.0 {
            format!(" · {}", CostCalculator::format_cost(self.turn_cost))
        } else {
            String::new()
        };

        Some(format!("Turn {elapsed} · {tool_text} · {token_text}{cost_text}"))
    }

    /// Update status bar to reflect auto-mode state.
    fn update_auto_mode_status(&mut self) {
        let on = self.auto_mode.load(Ordering::Relaxed);
        self.status_text =
            if on { format!("{} · auto", self.base_status) } else { self.base_status.clone() };

        // Persist to config.
        if let Some(base) = dirs::config_dir() {
            let path = base.join("telos").join("config.toml");
            let _ = save_auto_mode(&path, on);
        }
    }

    /// Send a user prompt to the background agent task.
    pub async fn send_prompt(&mut self, prompt: String) {
        if self.turn_active && self.cancellation.is_cancelled() {
            self.input.restore_text(prompt);
            self.status_text = "cancelling…".to_string();
            return;
        }
        crate::memory_runtime::record_user_preference(&self.memory, &prompt).await;
        self.input.record_history(prompt.clone());
        self.chat.push_entry(ChatEntry::user(prompt.clone()));
        let _ = self.turn_tx.send(BackgroundCommand::Prompt(prompt));
        if self.turn_active {
            self.status_text = "input queued for rethink".to_string();
        } else {
            self.cancellation.reset();
            self.base_status = self.status_text.trim_end_matches(" · auto").to_string();
            self.mode = Mode::Streaming;
            self.turn_active = true;
        }
    }

    /// Draw the entire UI.
    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let theme = Theme::default();

        // Layout: chat | approval | activity-line | input | bottom-bar
        let approval_height = if let Some(pending) = &self.inline_approval {
            approval_inline::inline_approval_height(
                pending,
                area.width as usize,
                self.inline_approval_expanded,
            )
        } else {
            0
        };
        let input_height = self.input_panel_height(area.width as usize);
        let show_activity = self.turn_active;
        let activity_height = if show_activity { 1 } else { 0 };
        let constraints = vec![
            Constraint::Min(0),                  // chat
            Constraint::Length(approval_height), // pending approval
            Constraint::Length(activity_height), // turn activity line
            Constraint::Length(input_height),    // input panel
            Constraint::Length(1),               // bottom bar
        ];

        let layout =
            Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);

        self.chat.render(frame, layout[0], &theme);
        if let Some(pending) = &self.inline_approval {
            self.inline_approval_area = Some(layout[1]);
            approval_inline::render(
                frame,
                layout[1],
                &theme,
                pending,
                self.inline_approval_expanded,
            );
        } else {
            self.inline_approval_area = None;
        }

        // ── Turn activity line (above input) ─────────────────────────
        if show_activity {
            let activity_idx = if self.inline_approval.is_some() { 2 } else { 1 };
            let elapsed = self.format_elapsed();
            let tokens = self.format_token_usage();
            let detail = if tokens.is_empty() {
                format!("{} ({})", self.status_text, elapsed)
            } else {
                format!("{} ({} · {})", self.status_text, elapsed, tokens)
            };
            let spinner_char =
                status_bar::SPINNER_CHARS[self.spinner_frame % status_bar::SPINNER_CHARS.len()];
            let line = ratatui::text::Line::from(ratatui::text::Span::styled(
                format!(" {} {}", spinner_char, detail),
                ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(138, 150, 170)),
            ));
            frame.render_widget(ratatui::widgets::Paragraph::new(line), layout[activity_idx]);
        }

        let input_idx = if show_activity {
            if self.inline_approval.is_some() { 3 } else { 2 }
        } else {
            if self.inline_approval.is_some() { 2 } else { 1 }
        };
        let bar_idx = input_idx + 1;

        self.input.render(
            frame,
            layout[input_idx],
            &theme,
            self.mode != Mode::Approving,
            self.mode == Mode::Streaming,
            self.spinner_frame,
        );

        // ── Bottom bar (permanent info) ───────────────────────────────
        status_bar::render(
            frame,
            layout[bar_idx],
            &self.status_text,
            self.spinner_frame,
            self.turn_total_tokens.unwrap_or(self.turn_input_tokens + self.turn_output_tokens),
            self.token_budget_max,
        );

        // ── Render active overlay on top ─────────────────────────────
        if let Some(overlay) = self.overlays.last() {
            overlay.render(frame, area, &theme);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TuiDensity;
    use crate::tui::chat_entry::ChatEntry;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use telos_agent::{ApprovalDecision, ApprovalRequest, Message, TurnEvent};
    use tokio::sync::oneshot;

    fn approval_request(command: &str) -> ApprovalRequest {
        ApprovalRequest {
            tool_name: "Bash".into(),
            invocation_names: vec!["Bash".into(), "shell".into()],
            arguments: json!({ "command": command }),
            cwd: PathBuf::from("."),
            messages: Arc::new(vec![Message::user("run a command")]),
            reason: "command requires approval".into(),
        }
    }

    fn test_app(temp: &tempfile::TempDir) -> App {
        let config = telos_agent::AgentConfig::default();
        let provider = Arc::new(telos_agent::MockProvider::new(vec![]));
        let tools = telos_agent::ToolRegistry::new();
        let memory = Arc::new(Mutex::new(MemoryStore::new(temp.path().join("memory"))));

        App::new(
            config,
            provider,
            tools,
            "telos".into(),
            Some(temp.path()),
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn layout_settings_map_density_presets() {
        assert_eq!(
            TuiLayoutSettings::from_density(TuiDensity::Compact),
            TuiLayoutSettings { input_height: 4 }
        );
        assert_eq!(
            TuiLayoutSettings::from_density(TuiDensity::Default),
            TuiLayoutSettings { input_height: 5 }
        );
        assert_eq!(
            TuiLayoutSettings::from_density(TuiDensity::Spacious),
            TuiLayoutSettings { input_height: 8 }
        );
    }

    #[tokio::test]
    async fn input_panel_height_grows_with_composer_content() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);

        assert_eq!(app.input_panel_height(80), 5);

        app.input.restore_text("line one\nline two\nline three".into());

        assert_eq!(app.input_panel_height(80), 6);
    }

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
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap();

        app.send_prompt("hello".into()).await;

        assert!(!cancelled.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn ctrl_up_recalls_prompt_sent_in_current_session() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);

        app.send_prompt("first prompt".into()).await;
        app.turn_active = false;
        app.mode = Mode::Normal;

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL)))
            .await
            .unwrap();

        assert_eq!(app.input.text(), "first prompt");
    }

    #[tokio::test]
    async fn new_session_clears_input_history() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);
        app.input.record_history("old prompt".into());

        app.new_session();
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL)))
            .await
            .unwrap();

        assert_eq!(app.input.text(), "");
    }

    #[tokio::test]
    async fn resume_session_rebuilds_input_history_from_user_messages() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);
        app.storage
            .save_snapshot(
                "saved-session",
                &[
                    Message::user("first prompt"),
                    Message::assistant("first answer"),
                    Message::user("latest prompt"),
                ],
            )
            .await
            .unwrap();

        app.resume_session("saved-session").await;
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL)))
            .await
            .unwrap();
        assert_eq!(app.input.text(), "latest prompt");

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL)))
            .await
            .unwrap();
        assert_eq!(app.input.text(), "first prompt");
    }

    #[tokio::test]
    async fn ctrl_c_marks_active_turn_as_cancelling_and_clears_composer() {
        let cancelled = Arc::new(AtomicBool::new(false));
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
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap();
        app.mode = Mode::Streaming;
        app.turn_active = true;
        app.status_text = "running tool".to_string();
        app.input.restore_text("draft to clear".into());

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)))
            .await
            .unwrap();

        assert!(cancelled.load(Ordering::Relaxed));
        assert!(app.turn_active);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.status_text, "cancelling…");
        assert_eq!(app.input.text(), "");
        assert!(!app.chat.has_active_assistant());
    }

    #[tokio::test]
    async fn submit_while_cancellation_is_pending_restores_draft() {
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
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap();
        app.turn_active = true;
        app.mode = Mode::Normal;

        app.send_prompt("gold price last 3 days".into()).await;

        assert_eq!(app.chat.len(), 0);
        assert_eq!(app.input.text(), "gold price last 3 days");
        assert_eq!(app.status_text, "cancelling…");
    }

    #[tokio::test]
    async fn assistant_delta_after_cancel_uses_new_cell() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);
        app.turn_active = true;
        app.mode = Mode::Streaming;
        app.handle_turn_event(TurnEvent::AssistantDelta { text: "partial".into() }).await;
        assert_eq!(app.chat.len(), 1);

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)))
            .await
            .unwrap();
        app.handle_turn_event(TurnEvent::AssistantDelta { text: "fresh".into() }).await;

        assert_eq!(app.chat.len(), 2);
    }

    #[tokio::test]
    async fn paste_event_inserts_text_into_composer() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);

        app.handle_event(Event::Paste("hello\nworld".into())).await.unwrap();

        assert_eq!(app.input.text(), "hello\nworld");
    }

    #[tokio::test]
    async fn submit_during_active_turn_is_accepted_for_rethink() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);
        app.turn_active = true;
        app.mode = Mode::Streaming;
        app.status_text = "running tool".to_string();

        app.handle_input_event(crate::tui::input_panel::InputEvent::Submit(
            "consider this too".into(),
        ))
        .await;

        assert!(app.turn_active);
        assert_eq!(app.mode, Mode::Streaming);
        assert_eq!(app.status_text, "input queued for rethink");
    }

    #[tokio::test]
    async fn ctrl_l_with_extra_shift_modifier_clears_chat() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);
        app.chat.push_entry(ChatEntry::separator());

        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('L'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )))
        .await
        .unwrap();

        assert_eq!(app.chat.len(), 0);
    }

    #[tokio::test]
    async fn shift_tab_encoded_as_shift_modified_tab_toggles_auto_mode() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)))
            .await
            .unwrap();

        assert!(app.auto_mode.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn empty_composer_down_scrolls_chat_when_not_browsing_history() {
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
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap();
        app.input.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('h'),
            crossterm::event::KeyModifiers::NONE,
        ));
        app.input.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        app.chat.scroll_offset = 3;

        app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        )))
        .await
        .unwrap();

        assert_eq!(app.chat.scroll_offset, 2);
    }

    #[tokio::test]
    async fn enqueue_pending_approval_sets_active_without_approving_mode() {
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
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap();
        let (tx, _rx) = oneshot::channel();

        app.enqueue_inline_approval(PendingApproval {
            request: approval_request("rm target"),
            respond: Some(tx),
        });

        assert!(app.inline_approval.is_some());
        assert_eq!(app.inline_approval_queue.len(), 0);
        assert_ne!(app.mode, Mode::Approving);
    }

    #[tokio::test]
    async fn inline_approval_shortcuts_resolve_allow_and_deny() {
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
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap();
        let (allow_tx, allow_rx) = oneshot::channel();
        app.enqueue_inline_approval(PendingApproval {
            request: approval_request("echo allow"),
            respond: Some(allow_tx),
        });

        app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::NONE,
        )))
        .await
        .unwrap();

        assert_eq!(allow_rx.await.unwrap(), ApprovalDecision::Allow);
        assert!(app.inline_approval.is_none());

        let (deny_tx, deny_rx) = oneshot::channel();
        app.enqueue_inline_approval(PendingApproval {
            request: approval_request("echo deny"),
            respond: Some(deny_tx),
        });

        app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('n'),
            crossterm::event::KeyModifiers::NONE,
        )))
        .await
        .unwrap();

        assert!(matches!(
            deny_rx.await.unwrap(),
            ApprovalDecision::Deny { reason } if reason == "denied by user"
        ));
        assert!(app.inline_approval.is_none());
    }

    #[tokio::test]
    async fn inline_approval_t_key_toggles_command_expansion() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);
        let (tx, _rx) = oneshot::channel();
        app.enqueue_inline_approval(PendingApproval {
            request: approval_request(
                "find . -maxdepth 2 -type f -name \"*.md\" -o -name \"*.py\"",
            ),
            respond: Some(tx),
        });

        assert!(!app.inline_approval_expanded);

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)))
            .await
            .unwrap();

        assert!(app.inline_approval_expanded);

        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)))
            .await
            .unwrap();

        assert!(!app.inline_approval_expanded);
    }

    #[tokio::test]
    async fn clicking_inline_approval_command_toggles_expansion() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);
        let (tx, _rx) = oneshot::channel();
        app.enqueue_inline_approval(PendingApproval {
            request: approval_request(
                "find . -maxdepth 2 -type f -name \"*.md\" -o -name \"*.py\"",
            ),
            respond: Some(tx),
        });

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let area = app.inline_approval_area.expect("approval area should be tracked after draw");

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 3,
            row: area.y + 3,
            modifiers: KeyModifiers::NONE,
        }))
        .await
        .unwrap();
        assert!(!app.inline_approval_expanded);

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 3,
            row: area.y + 2,
            modifiers: KeyModifiers::NONE,
        }))
        .await
        .unwrap();
        assert!(app.inline_approval_expanded);
    }

    #[tokio::test]
    async fn mouse_wheel_scrolls_chat_history() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = test_app(&temp);
        app.chat.scroll_offset = 3;

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 10,
            row: 10,
            modifiers: KeyModifiers::NONE,
        }))
        .await
        .unwrap();
        assert_eq!(app.chat.scroll_offset, 4);

        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 10,
            modifiers: KeyModifiers::NONE,
        }))
        .await
        .unwrap();
        assert_eq!(app.chat.scroll_offset, 3);
    }

    #[tokio::test]
    async fn enqueue_pending_approvals_keeps_fifo_order() {
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
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap();
        let (first_tx, _first_rx) = oneshot::channel();
        let (second_tx, _second_rx) = oneshot::channel();

        app.enqueue_inline_approval(PendingApproval {
            request: approval_request("first"),
            respond: Some(first_tx),
        });
        app.enqueue_inline_approval(PendingApproval {
            request: approval_request("second"),
            respond: Some(second_tx),
        });

        assert_eq!(app.inline_approval.as_ref().unwrap().request.arguments["command"], "first");
        assert_eq!(app.inline_approval_queue.len(), 1);
    }

    #[tokio::test]
    async fn approval_channel_tick_uses_inline_state_instead_of_overlay() {
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
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap();
        let (tx, _rx) = oneshot::channel();
        let (approval_tx, approval_rx) = tokio::sync::mpsc::unbounded_channel();
        approval_tx
            .send(PendingApproval { request: approval_request("echo inline"), respond: Some(tx) })
            .unwrap();
        app.approval_rx = approval_rx;

        app.handle_event(Event::Tick).await.unwrap();

        assert!(app.inline_approval.is_some());
        assert!(app.overlays.is_empty());
        assert_ne!(app.mode, Mode::Approving);
    }

    #[tokio::test]
    async fn streaming_character_input_updates_composer() {
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
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap();
        app.mode = Mode::Streaming;
        app.turn_active = true;

        app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('h'),
            crossterm::event::KeyModifiers::NONE,
        )))
        .await
        .unwrap();
        app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('i'),
            crossterm::event::KeyModifiers::NONE,
        )))
        .await
        .unwrap();

        assert_eq!(app.input.text(), "hi");
        assert_eq!(app.mode, Mode::Streaming);
        assert!(app.turn_active);
    }

    #[tokio::test]
    async fn streaming_enter_submits_prompt_for_rethink() {
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
            temp.path(),
            false,
            memory,
            ModelSwitchConfig::default(),
            None,
        )
        .unwrap();
        app.mode = Mode::Streaming;
        app.turn_active = true;
        app.input.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('h'),
            crossterm::event::KeyModifiers::NONE,
        ));
        app.input.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('i'),
            crossterm::event::KeyModifiers::NONE,
        ));

        app.handle_event(Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        )))
        .await
        .unwrap();

        assert_eq!(app.input.text(), "");
        assert_eq!(app.chat.len(), 1);
        assert_eq!(app.status_text, "input queued for rethink");
        assert_eq!(app.mode, Mode::Streaming);
    }
}
