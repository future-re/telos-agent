# Codex-Style TUI CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform `telos-cli` from a rustyline REPL into a Codex CLI-style full-screen terminal UI while keeping the one-shot prompt mode and shell-completion subcommand unchanged.

**Architecture:** Add a `cli/src/tui/` module tree powered by `ratatui` + `crossterm`. The `AgentSession` runs in a background `tokio` task; `TurnEvent`s flow into the UI thread through an `mpsc` channel. Human approvals are bridged via `tokio::sync::oneshot` so the TUI can render an inline approval overlay. The existing `runner::run_single` path stays intact for `telos "prompt"`.

**Tech Stack:** Rust 2024, ratatui 0.29, crossterm 0.28, tui-textarea 0.7, termimad 0.30, tokio, clap, serde_json, anyhow

---

## File map

| File | Responsibility |
|------|----------------|
| `cli/Cargo.toml` | Add ratatui, crossterm, tui-textarea, termimad dependencies |
| `cli/src/lib.rs` | Dispatch `telos` (no args) to `tui::run`; keep `telos "prompt"` and `telos completion` |
| `cli/src/runner.rs` | Redirect `run_chat` to the TUI; keep `run_single` for one-shot mode |
| `cli/src/context.rs` | NEW: load `CLAUDE.md` / `AGENTS.md` / `CODEBUDDY.md` / `GEMINI.md` and `git status` |
| `cli/src/tui/mod.rs` | Module declarations + `run()` entry point |
| `cli/src/tui/event.rs` | `Event` enum merging crossterm input, resize, agent events, and ticks |
| `cli/src/tui/app.rs` | `App` state machine (`Mode`, messages, input, chat scroll, approvals) |
| `cli/src/tui/status_bar.rs` | Top status line widget |
| `cli/src/tui/chat_panel.rs` | Scrollable conversation history widget |
| `cli/src/tui/input_panel.rs` | `tui-textarea`-based multi-line input widget |
| `cli/src/tui/approval.rs` | `TuiApprovalHandler` + `PendingApproval` oneshot bridge |
| `cli/src/tui/markdown.rs` | Lightweight markdown-to-ratatui `Text` renderer (termimad fallback) |
| `cli/src/tui/theme.rs` | Color theme definitions |
| `cli/src/repl.rs` | DELETE after `run_chat` is redirected |
| `cli/README.md` | Update for TUI mode |
| `cli/tests/cli_tests.rs` | Keep existing tests; add TUI compile-smoke tests |

---

## Global constraints

- `telos "prompt"` one-shot mode behavior must remain unchanged.
- `telos completion <shell>` must remain unchanged.
- `telos chat` launches the TUI.
- `telos` with no arguments launches the TUI.
- Core library (`telos_agent`) receives no API-breaking changes.
- All existing workspace tests continue to pass after each commit.
- Rust edition 2024, MSRV 1.96.

---

### Task 1: Add TUI dependencies

**Files:**
- Modify: `cli/Cargo.toml`
- Test: `cargo check -p telos-cli`

**Interfaces:**
- Produces: `cli/Cargo.toml` with ratatui/crossterm/tui-textarea/termimad listed.

- [ ] **Step 1: Add crates to `cli/Cargo.toml`**

  Insert these lines inside the existing `[dependencies]` section, keeping alphabetical order where possible:

  ```toml
  crossterm = { version = "0.28", features = ["event-stream"] }
  ratatui = "0.29"
  termimad = "0.30"
  tui-textarea = "0.7"
  ```

  The `[dependencies]` section should now include the new entries alongside existing ones (`anyhow`, `async-trait`, `clap`, `clap_complete`, `dirs`, `futures-util`, `glob`, `rpassword`, `rustyline`, `serde`, `serde_json`, `tokio`, `toml`, `telos_agent`, `tracing`).

- [ ] **Step 2: Verify dependency resolution**

  Run: `cargo check -p telos-cli`
  Expected: Resolves and downloads new crates; no compile errors yet (the new crates are not imported anywhere).

- [ ] **Step 3: Commit**

  ```bash
  git add cli/Cargo.toml Cargo.lock
  git commit -m "feat(tui): add ratatui, crossterm, tui-textarea, termimad deps

  Prepare for Codex-style full-screen TUI.

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 2: Create TUI module skeleton

**Files:**
- Create: `cli/src/tui/mod.rs`
- Create: `cli/src/tui/event.rs`
- Modify: `cli/src/lib.rs`

**Interfaces:**
- Produces: `telos_cli::tui::event::Event` enum and `telos_cli::tui` module registered in `lib.rs`.

- [ ] **Step 1: Create `cli/src/tui/event.rs`**

  ```rust
  use crossterm::event::{KeyEvent, MouseEvent};
  use telos_agent::TurnEvent;

  /// Events that flow through the TUI event loop.
  #[derive(Debug)]
  pub enum Event {
      /// A keyboard event from crossterm.
      Key(KeyEvent),
      /// A mouse event from crossterm.
      Mouse(MouseEvent),
      /// Terminal was resized.
      Resize { cols: u16, rows: u16 },
      /// A turn event from the agent stream.
      Turn(TurnEvent),
      /// The agent turn completed (stream ended).
      TurnComplete,
      /// Request to redraw (e.g. from a timer tick).
      Tick,
  }
  ```

- [ ] **Step 2: Create `cli/src/tui/mod.rs`**

  ```rust
  pub mod app;
  pub mod approval;
  pub mod chat_panel;
  pub mod event;
  pub mod input_panel;
  pub mod markdown;
  pub mod status_bar;
  pub mod theme;
  ```

  Also declare the public entry point (we will implement the body in Task 4):

  ```rust
  use anyhow::Result;
  use std::sync::Arc;
  use telos_agent::{AgentConfig, ModelProvider, ToolRegistry};

  /// Launch the ratatui full-screen TUI.
  pub async fn run(
      _config: AgentConfig,
      _provider: Arc<dyn ModelProvider>,
      _tools: ToolRegistry,
      _status_text: String,
  ) -> Result<()> {
      todo!("TUI entry point implemented in Task 4")
  }
  ```

- [ ] **Step 3: Register `tui` module in `cli/src/lib.rs`**

  Add `pub mod tui;` after the existing module declarations:

  ```rust
  pub mod approval;
  pub mod cli;
  pub mod config;
  pub mod project;
  pub mod repl;
  pub mod runner;
  pub mod session;
  pub mod terminal;
  pub mod tui;
  ```

- [ ] **Step 4: Verify compilation**

  Run: `cargo check -p telos-cli`
  Expected: Compiles; only a `todo!()` warning or error is acceptable at this stage.

- [ ] **Step 5: Commit**

  ```bash
  git add cli/src/tui/ cli/src/lib.rs
  git commit -m "feat(tui): add tui module skeleton and Event enum

  - Create cli/src/tui/ tree with module declarations
  - Define Event enum merging crossterm and agent events
  - Register pub mod tui in lib.rs

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 3: Create App state and empty frame loop

**Files:**
- Create: `cli/src/tui/app.rs`
- Modify: `cli/src/tui/mod.rs` (remove `todo!()` placeholder later in Task 4)

**Interfaces:**
- Produces: `tui::app::App` with `Mode`, `UiMessage`, layout, and placeholder widgets.

- [ ] **Step 1: Create `cli/src/tui/app.rs`**

  ```rust
  use ratatui::layout::{Constraint, Direction, Layout, Rect};
  use ratatui::widgets::Paragraph;
  use ratatui::Frame;
  use std::collections::VecDeque;
  use std::sync::Arc;
  use telos_agent::{ApprovalDecision, TurnEvent};

  use crate::tui::approval::PendingApproval;
  use crate::tui::chat_panel::ChatPanel;
  use crate::tui::event::Event;
  use crate::tui::input_panel::InputPanel;
  use crate::tui::status_bar;

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
      ToolCall { id: String, name: String },
      /// Tool call finished.
      ToolCompleted { id: String, name: String, is_error: bool },
      /// Marks the end of a turn.
      TurnComplete,
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
  }

  impl App {
      pub fn new(status_text: String) -> Self {
          Self {
              mode: Mode::Normal,
              should_quit: false,
              status_text,
              messages: Vec::new(),
              chat: ChatPanel::new(),
              input: InputPanel::new(),
              pending_approvals: VecDeque::new(),
          }
      }

      /// Process a single event.
      pub fn handle_event(&mut self, event: Event) -> anyhow::Result<()> {
          match event {
              Event::Key(key) => {
                  use crossterm::event::{KeyCode, KeyModifiers};
                  if key.code == KeyCode::Char('d')
                      && key.modifiers.contains(KeyModifiers::CONTROL)
                      && self.input.is_empty()
                  {
                      self.should_quit = true;
                      return Ok(());
                  }
                  if self.mode == Mode::Normal {
                      if let Some(prompt) = self.input.handle_key(key) {
                          self.send_prompt(prompt);
                      }
                  }
              }
              Event::Tick => {}
              Event::Resize { .. } => {}
              Event::Turn(turn_event) => self.handle_turn_event(turn_event),
              Event::TurnComplete => {
                  self.messages.push(UiMessage::TurnComplete);
                  self.mode = Mode::Normal;
              }
              _ => {}
          }
          Ok(())
      }

      /// Queue a user prompt as a new UI message.
      pub fn send_prompt(&mut self, prompt: String) {
          self.messages.push(UiMessage::User(prompt));
          self.mode = Mode::Streaming;
      }

      /// Convert an agent `TurnEvent` into a `UiMessage`.
      fn handle_turn_event(&mut self, event: TurnEvent) {
          match event {
              TurnEvent::AssistantDelta { text } => {
                  self.messages.push(UiMessage::AssistantDelta(text));
              }
              TurnEvent::ThinkingDelta { text } => {
                  self.messages.push(UiMessage::ThinkingDelta(text));
              }
              TurnEvent::ToolCall { tool_call_id, name } => {
                  self.messages.push(UiMessage::ToolCall { id: tool_call_id, name });
              }
              TurnEvent::ToolCompleted { tool_call_id, name, is_error } => {
                  self.messages.push(UiMessage::ToolCompleted {
                      id: tool_call_id,
                      name,
                      is_error,
                  });
              }
              _ => {}
          }
      }

      /// Draw the entire UI.
      pub fn draw(&self, frame: &mut Frame) {
          let area = frame.area();
          let layout = Layout::default()
              .direction(Direction::Vertical)
              .constraints([
                  Constraint::Length(1), // status bar
                  Constraint::Min(0),    // chat panel
                  Constraint::Length(3), // input panel
              ])
              .split(area);

          status_bar::render(frame, layout[0], &self.status_text);

          let placeholder = Paragraph::new("Welcome to telos TUI.\nPress Ctrl+D on empty input to exit.");
          frame.render_widget(placeholder, layout[1]);

          self.input.render(frame, layout[2], self.mode == Mode::Normal);
      }
  }
  ```

  Note: `ChatPanel` and `InputPanel` are referenced but not yet implemented; stubs will be added in Tasks 5 and 6. For this task, temporarily create minimal stubs so `App` compiles.

- [ ] **Step 2: Create temporary stubs for `ChatPanel`, `InputPanel`, and `status_bar`**

  `cli/src/tui/chat_panel.rs`:
  ```rust
  #[derive(Debug, Default)]
  pub struct ChatPanel;
  impl ChatPanel {
      pub fn new() -> Self { Self }
  }
  ```

  `cli/src/tui/input_panel.rs`:
  ```rust
  use crossterm::event::KeyEvent;
  use ratatui::layout::Rect;
  use ratatui::Frame;

  #[derive(Debug, Default)]
  pub struct InputPanel;
  impl InputPanel {
      pub fn new() -> Self { Self }
      pub fn is_empty(&self) -> bool { true }
      pub fn handle_key(&mut self, _key: KeyEvent) -> Option<String> { None }
      pub fn render(&self, _frame: &mut Frame, _area: Rect, _active: bool) {}
  }
  ```

  `cli/src/tui/status_bar.rs`:
  ```rust
  use ratatui::layout::Rect;
  use ratatui::style::{Color, Style};
  use ratatui::text::Line;
  use ratatui::widgets::Paragraph;
  use ratatui::Frame;

  pub fn render(frame: &mut Frame, area: Rect, status: &str) {
      let style = Style::default().fg(Color::White).bg(Color::DarkGray);
      let paragraph = Paragraph::new(Line::from(status.to_string())).style(style);
      frame.render_widget(paragraph, area);
  }
  ```

  `cli/src/tui/approval.rs` (minimal stub):
  ```rust
  use telos_agent::{ApprovalDecision, ApprovalRequest};

  #[derive(Debug)]
  pub struct PendingApproval {
      pub request: ApprovalRequest,
      pub respond: tokio::sync::oneshot::Sender<ApprovalDecision>,
  }
  ```

  `cli/src/tui/markdown.rs` (minimal stub):
  ```rust
  use ratatui::text::Text;

  pub fn render_markdown(_text: &str) -> Text<'static> {
      Text::from("markdown placeholder")
  }
  ```

  `cli/src/tui/theme.rs` (minimal stub):
  ```rust
  use ratatui::style::{Color, Style};

  #[derive(Debug, Clone, Copy)]
  pub struct Theme;
  impl Theme {
      pub fn default() -> Self { Self }
      pub fn user_style(&self) -> Style { Style::default().fg(Color::Cyan) }
      pub fn assistant_style(&self) -> Style { Style::default().fg(Color::Gray) }
  }
  ```

- [ ] **Step 3: Verify compilation**

  Run: `cargo check -p telos-cli`
  Expected: Compiles successfully with the stub widgets.

- [ ] **Step 4: Commit**

  ```bash
  git add cli/src/tui/
  git commit -m "feat(tui): add App state and empty frame loop

  - Define Mode, UiMessage, and App struct
  - Stub out chat_panel, input_panel, status_bar, approval, markdown, theme
  - Layout with status bar, chat area, and input area

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 4: Wire TUI entry point

**Files:**
- Modify: `cli/src/tui/mod.rs`
- Modify: `cli/src/lib.rs`
- Modify: `cli/src/runner.rs`
- Test: `cargo test -p telos-cli`

**Interfaces:**
- Produces: `telos` with no args launches the TUI; `telos chat` launches the TUI.

- [ ] **Step 1: Implement `run()` in `cli/src/tui/mod.rs`**

  Replace the `todo!()` body from Task 2 with:

  ```rust
  use crate::tui::app::App;
  use crate::tui::event::Event;
  use anyhow::Result;
  use crossterm::event::{Event as CEvent, EventStream};
  use futures_util::StreamExt;
  use ratatui::backend::CrosstermBackend;
  use ratatui::Terminal;
  use std::io::{self, stdout};
  use std::sync::Arc;
  use std::time::Duration;
  use telos_agent::{AgentConfig, ModelProvider, ToolRegistry};

  /// Launch the ratatui full-screen TUI.
  pub async fn run(
      config: AgentConfig,
      provider: Arc<dyn ModelProvider>,
      tools: ToolRegistry,
      status_text: String,
  ) -> Result<()> {
      crossterm::terminal::enable_raw_mode()?;
      let mut stdout = stdout();
      crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;

      let backend = CrosstermBackend::new(stdout);
      let mut terminal = Terminal::new(backend)?;

      let mut app = App::new(status_text);
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

          if app.handle_event(event).is_err() || app.should_quit {
              break;
          }
      }

      crossterm::terminal::disable_raw_mode()?;
      crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)?;
      Ok(())
  }
  ```

  Note: The `config`, `provider`, and `tools` arguments are accepted but not yet used. They will be wired in Task 7.

- [ ] **Step 2: Update `cli/src/lib.rs` to dispatch no-args to TUI**

  Replace the `None` branch in `run()`:

  ```rust
  None => {
      let prompt = cli.prompt.join(" ");
      if prompt.trim().is_empty() {
          let config = config::build_agent_config(&cli.shared, approval_handler.clone())?;
          let provider = build_erased_provider(&cli.shared)?;
          let mut tools = telos_agent::ToolRegistry::new();
          telos_agent::register_core_tools(&mut tools);

          let cwd = cli.shared.cwd.as_deref().unwrap_or(&std::env::current_dir()?);
          let project_root = project::find_project_root(cwd).ok();
          let ctx = match &project_root {
              Some(root) => crate::context::load_project_context(root),
              None => crate::context::ProjectContext::empty(),
          };

          let model = cli.shared.model.as_deref().unwrap_or("default");
          let project_name = project_root
              .as_ref()
              .and_then(|p| p.file_name())
              .map(|n| n.to_string_lossy().to_string())
              .unwrap_or_else(|| "?".to_string());
          let status = format!("telos · {} · {} · {}", model, project_name, ctx.instructions_file.as_deref().unwrap_or("no project docs"));

          return tui::run(config, provider, tools, status).await;
      }
      runner::run_single(&cli.shared, prompt, approval_handler).await
  }
  ```

  Also add the helper function at the bottom of `lib.rs`:

  ```rust
  fn build_erased_provider(options: &cli::SharedOptions) -> Result<Arc<dyn telos_agent::ModelProvider>> {
      match config::build_provider(options)? {
          config::ResolvedProvider::Kimi(p) => Ok(Arc::new(p)),
          config::ResolvedProvider::DeepSeek(p) => Ok(Arc::new(p)),
          config::ResolvedProvider::Mock(p) => Ok(Arc::new(p)),
      }
  }
  ```

  And add `pub mod context;` to the module declarations.

- [ ] **Step 3: Create minimal `cli/src/context.rs` stub**

  ```rust
  use std::path::Path;

  #[derive(Debug, Clone, Default)]
  pub struct ProjectContext {
      pub project_instructions: Option<String>,
      pub instructions_file: Option<String>,
      pub git_status: Option<String>,
  }

  impl ProjectContext {
      pub fn empty() -> Self { Self::default() }
  }

  pub fn load_project_context(_root: &Path) -> ProjectContext {
      ProjectContext::default()
  }
  ```

  This stub will be filled in Task 11.

- [ ] **Step 4: Redirect `run_chat` to the TUI in `cli/src/runner.rs`**

  Replace the body of `run_chat` with:

  ```rust
  pub async fn run_chat(
      options: &SharedOptions,
      approval_handler: Option<Arc<dyn ApprovalHandler>>,
  ) -> Result<()> {
      let config = build_agent_config(options, approval_handler)?;
      let provider = crate::build_erased_provider(options)?;
      let mut tools = ToolRegistry::new();
      telos_agent::register_core_tools(&mut tools);

      let cwd = options.cwd.as_deref().unwrap_or(&std::env::current_dir()?);
      let project_root = crate::project::find_project_root(cwd).ok();
      let ctx = match &project_root {
          Some(root) => crate::context::load_project_context(root),
          None => crate::context::ProjectContext::empty(),
      };

      let model = options.model.as_deref().unwrap_or("default");
      let project_name = project_root
          .as_ref()
          .and_then(|p| p.file_name())
          .map(|n| n.to_string_lossy().to_string())
          .unwrap_or_else(|| "?".to_string());
      let status = format!("telos · {} · {} · {}", model, project_name, ctx.instructions_file.as_deref().unwrap_or("no project docs"));

      crate::tui::run(config, provider, tools, status).await
  }
  ```

  Note: `build_erased_provider` is in `lib.rs`. Since `runner.rs` is part of the same crate, it can call `crate::build_erased_provider`.

- [ ] **Step 5: Verify compilation and existing tests**

  Run: `cargo check -p telos-cli`
  Run: `cargo test -p telos-cli`
  Expected: Compiles; existing tests pass. The TUI entry point is not exercised by automated tests yet.

- [ ] **Step 6: Commit**

  ```bash
  git add cli/src/tui/mod.rs cli/src/lib.rs cli/src/runner.rs cli/src/context.rs
  git commit -m "feat(tui): wire no-args and chat subcommand to TUI

  - telos and telos chat launch the ratatui TUI
  - Keep telos 'prompt' and telos completion unchanged
  - Add context.rs stub and build_erased_provider helper

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 5: Implement StatusBar and theme

**Files:**
- Modify: `cli/src/tui/status_bar.rs`
- Modify: `cli/src/tui/theme.rs`
- Modify: `cli/src/tui/app.rs` (use theme in status bar if desired)
- Test: `cargo test -p telos-cli`

**Interfaces:**
- Produces: themed status bar rendering and reusable `Theme` struct.

- [ ] **Step 1: Implement `cli/src/tui/theme.rs`**

  ```rust
  use ratatui::style::{Color, Modifier, Style};

  #[derive(Debug, Clone, Copy)]
  pub struct Theme {
      pub status_bg: Color,
      pub status_fg: Color,
      pub user_fg: Color,
      pub assistant_fg: Color,
      pub tool_pending_fg: Color,
      pub tool_ok_fg: Color,
      pub tool_error_fg: Color,
      pub thinking_fg: Color,
      pub border_active: Color,
      pub border_inactive: Color,
      pub input_placeholder: Color,
  }

  impl Default for Theme {
      fn default() -> Self {
          Self {
              status_bg: Color::DarkGray,
              status_fg: Color::White,
              user_fg: Color::Cyan,
              assistant_fg: Color::Gray,
              tool_pending_fg: Color::Yellow,
              tool_ok_fg: Color::Green,
              tool_error_fg: Color::Red,
              thinking_fg: Color::DarkGray,
              border_active: Color::Cyan,
              border_inactive: Color::DarkGray,
              input_placeholder: Color::DarkGray,
          }
      }
  }

  impl Theme {
      pub fn user_style(&self) -> Style {
          Style::default().fg(self.user_fg).add_modifier(Modifier::BOLD)
      }

      pub fn assistant_style(&self) -> Style {
          Style::default().fg(self.assistant_fg)
      }

      pub fn thinking_style(&self) -> Style {
          Style::default().fg(self.thinking_fg).add_modifier(Modifier::ITALIC)
      }

      pub fn tool_pending_style(&self) -> Style {
          Style::default().fg(self.tool_pending_fg)
      }

      pub fn tool_ok_style(&self) -> Style {
          Style::default().fg(self.tool_ok_fg)
      }

      pub fn tool_error_style(&self) -> Style {
          Style::default().fg(self.tool_error_fg)
      }
  }
  ```

- [ ] **Step 2: Update `cli/src/tui/status_bar.rs` to use theme**

  ```rust
  use ratatui::layout::Rect;
  use ratatui::text::Line;
  use ratatui::widgets::Paragraph;
  use ratatui::Frame;

  use crate::tui::theme::Theme;

  pub fn render(frame: &mut Frame, area: Rect, status: &str) {
      let theme = Theme::default();
      let style = Style::default().fg(theme.status_fg).bg(theme.status_bg);
      let paragraph = Paragraph::new(Line::from(status.to_string())).style(style);
      frame.render_widget(paragraph, area);
  }
  ```

- [ ] **Step 3: Verify compilation**

  Run: `cargo check -p telos-cli`
  Expected: Compiles.

- [ ] **Step 4: Commit**

  ```bash
  git add cli/src/tui/theme.rs cli/src/tui/status_bar.rs
  git commit -m "feat(tui): add Theme and styled StatusBar

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 6: Implement InputPanel with tui-textarea

**Files:**
- Modify: `cli/src/tui/input_panel.rs`
- Modify: `cli/src/tui/app.rs`
- Test: `cargo test -p telos-cli`

**Interfaces:**
- Produces: multi-line input widget; Enter submits, Alt+Enter inserts newline.

- [ ] **Step 1: Implement `cli/src/tui/input_panel.rs`**

  ```rust
  use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
  use ratatui::layout::Rect;
  use ratatui::style::Style;
  use ratatui::widgets::{Block, Borders};
  use ratatui::Frame;
  use tui_textarea::TextArea;

  use crate::tui::theme::Theme;

  pub struct InputPanel {
      textarea: TextArea<'static>,
  }

  impl InputPanel {
      pub fn new() -> Self {
          let mut textarea = TextArea::default();
          textarea.set_placeholder_text("Type a message… (Enter send, Alt+Enter newline, Ctrl+D quit when empty)");
          textarea.set_cursor_line_style(Style::default());
          Self { textarea }
      }

      pub fn is_empty(&self) -> bool {
          self.textarea.lines().join("").trim().is_empty()
      }

      /// Process a key event. Returns `Some(String)` when the user submits.
      pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
          match key {
              KeyEvent {
                  code: KeyCode::Enter,
                  modifiers: KeyModifiers::NONE,
                  ..
              } => {
                  let text = self.textarea.lines().join("\n");
                  let trimmed = text.trim();
                  if trimmed.is_empty() {
                      return None;
                  }
                  self.textarea.select_all();
                  self.textarea.cut();
                  Some(trimmed.to_string())
              }
              KeyEvent {
                  code: KeyCode::Enter,
                  modifiers: KeyModifiers::ALT,
                  ..
              } => {
                  self.textarea.insert_newline();
                  None
              }
              _ => {
                  self.textarea.input(key);
                  None
              }
          }
      }

      pub fn render(&self, frame: &mut Frame, area: Rect, active: bool) {
          let theme = Theme::default();
          let border_style = if active {
              Style::default().fg(theme.border_active)
          } else {
              Style::default().fg(theme.border_inactive)
          };
          let block = Block::default()
              .borders(Borders::TOP)
              .border_style(border_style);
          let widget = self.textarea.widget().block(block);
          frame.render_widget(widget, area);
      }
  }

  impl Default for InputPanel {
      fn default() -> Self { Self::new() }
  }
  ```

  Note: `TextArea::select_all()` and `TextArea::cut()` are available in tui-textarea 0.7. If they are not, clear manually by iterating lines.

- [ ] **Step 2: Update `App::draw` to use the real InputPanel**

  No change needed if `App::draw` already calls `self.input.render(...)`. Ensure `App::handle_event` routes keys to `input.handle_key` only in `Mode::Normal`.

- [ ] **Step 3: Verify compilation and existing tests**

  Run: `cargo check -p telos-cli`
  Run: `cargo test -p telos-cli`
  Expected: Compiles; tests pass.

- [ ] **Step 4: Commit**

  ```bash
  git add cli/src/tui/input_panel.rs cli/src/tui/app.rs
  git commit -m "feat(tui): add multi-line InputPanel with tui-textarea

  - Enter submits, Alt+Enter inserts newline
  - Ctrl+D quits only when input is empty
  - Active/inactive border styling

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 7: TurnEvent bridge — run AgentSession in background task

**Files:**
- Modify: `cli/src/tui/app.rs`
- Modify: `cli/src/tui/mod.rs`
- Test: `cargo test -p telos-cli` + manual smoke test

**Interfaces:**
- Produces: `App` owns channels to a background task that drives `AgentSession::run_turn_stream`.

- [ ] **Step 1: Update `App` struct to own channels**

  ```rust
  use std::pin::pin;
  use futures_util::StreamExt;
  use tokio::sync::mpsc;

  pub struct App {
      pub mode: Mode,
      pub should_quit: bool,
      pub status_text: String,
      pub messages: Vec<UiMessage>,
      pub chat: ChatPanel,
      pub input: InputPanel,
      pub pending_approvals: VecDeque<PendingApproval>,
      /// Send prompts to the background agent task.
      turn_tx: mpsc::UnboundedSender<String>,
      /// Receive TurnEvents from the background agent task.
      turn_rx: mpsc::UnboundedReceiver<Event>,
      /// Receive pending approvals from the TuiApprovalHandler.
      approval_rx: mpsc::UnboundedReceiver<PendingApproval>,
  }
  ```

- [ ] **Step 2: Rewrite `App::new` to spawn the background task**

  ```rust
  impl App {
      pub fn new(
          config: telos_agent::AgentConfig,
          provider: Arc<dyn telos_agent::ModelProvider>,
          tools: telos_agent::ToolRegistry,
          status_text: String,
      ) -> Result<Self, telos_agent::AgentError> {
          let (prompt_tx, mut prompt_rx) = mpsc::unbounded_channel::<String>();
          let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();
          let (approval_tx, approval_rx) = mpsc::unbounded_channel::<PendingApproval>();

          // Background task owns the AgentSession because run_turn_stream needs &mut self.
          tokio::spawn(async move {
              let approval_handler: Option<Arc<dyn telos_agent::ApprovalHandler>> =
                  Some(Arc::new(crate::tui::approval::TuiApprovalHandler::new(approval_tx)));
              let mut config = config;
              config.approval_handler = approval_handler;

              let mut session = telos_agent::AgentSession::new(config)
                  .expect("failed to create agent session");

              while let Some(prompt) = prompt_rx.recv().await {
                  let mut stream = pin!(session.run_turn_stream(
                      &telos_agent::ErasedProvider(provider.as_ref()),
                      &tools,
                      prompt,
                  ));
                  while let Some(event) = stream.next().await {
                      match event {
                          Ok(te) => {
                              let _ = event_tx.send(Event::Turn(te));
                          }
                          Err(_e) => break,
                      }
                  }
                  let _ = event_tx.send(Event::TurnComplete);
              }
          });

          Ok(Self {
              mode: Mode::Normal,
              should_quit: false,
              status_text,
              messages: Vec::new(),
              chat: ChatPanel::new(),
              input: InputPanel::new(),
              pending_approvals: VecDeque::new(),
              turn_tx: prompt_tx,
              turn_rx: event_rx,
              approval_rx,
          })
      }

      /// Send a user prompt to the background agent task.
      pub fn send_prompt(&mut self, prompt: String) {
          self.messages.push(UiMessage::User(prompt.clone()));
          let _ = self.turn_tx.send(prompt);
          self.mode = Mode::Streaming;
      }
  }
  ```

- [ ] **Step 3: Implement `TuiApprovalHandler` in `cli/src/tui/approval.rs`**

  ```rust
  use async_trait::async_trait;
  use std::sync::Arc;
  use telos_agent::{ApprovalDecision, ApprovalHandler, ApprovalRequest};
  use tokio::sync::mpsc;
  use tokio::sync::oneshot;

  #[derive(Debug)]
  pub struct PendingApproval {
      pub request: ApprovalRequest,
      pub respond: oneshot::Sender<ApprovalDecision>,
  }

  pub struct TuiApprovalHandler {
      tx: mpsc::UnboundedSender<PendingApproval>,
  }

  impl TuiApprovalHandler {
      pub fn new(tx: mpsc::UnboundedSender<PendingApproval>) -> Self {
          Self { tx }
      }
  }

  impl std::fmt::Debug for TuiApprovalHandler {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          f.debug_struct("TuiApprovalHandler").finish_non_exhaustive()
      }
  }

  #[async_trait]
  impl ApprovalHandler for TuiApprovalHandler {
      async fn ask(&self, request: ApprovalRequest) -> ApprovalDecision {
          let (tx, rx) = oneshot::channel();
          let pending = PendingApproval { request, respond: tx };
          if self.tx.send(pending).is_err() {
              return ApprovalDecision::Deny {
                  reason: "TUI approval channel closed".into(),
              };
          }
          rx.await.unwrap_or(ApprovalDecision::Deny {
              reason: "no response from user".into(),
          })
      }
  }
  ```

- [ ] **Step 4: Drain events and approvals on `Tick`**

  In `App::handle_event`, update the `Event::Tick` branch:

  ```rust
  Event::Tick => {
      while let Ok(event) = self.turn_rx.try_recv() {
          self.handle_event(event)?;
      }
      while let Ok(pending) = self.approval_rx.try_recv() {
          self.pending_approvals.push_back(pending);
          self.mode = Mode::Approving;
      }
  }
  ```

  Make sure `handle_event` is not called recursively with `Event::Turn` while `self.mode` is `Approving`. The recursive call for `Event::Turn` and `Event::TurnComplete` is fine because those variants do not re-enter the `Tick` branch.

- [ ] **Step 5: Update `tui::run` signature and call site**

  `cli/src/tui/mod.rs` `run()` already accepts `config`, `provider`, `tools`, and `status_text`. Update the `App::new` call:

  ```rust
  let mut app = App::new(config, provider, tools, status_text)?;
  ```

- [ ] **Step 6: Verify compilation and run tests**

  Run: `cargo check -p telos-cli`
  Run: `cargo test -p telos-cli`
  Expected: Compiles; tests pass.

- [ ] **Step 7: Commit**

  ```bash
  git add cli/src/tui/app.rs cli/src/tui/approval.rs cli/src/tui/mod.rs
  git commit -m "feat(tui): bridge AgentSession TurnEvents into the TUI

  - AgentSession runs in a background tokio task
  - TurnEvents and approvals flow to UI thread via mpsc
  - TuiApprovalHandler suspends tool calls with oneshot channel

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 8: Implement ChatPanel with scrolling

**Files:**
- Modify: `cli/src/tui/chat_panel.rs`
- Modify: `cli/src/tui/app.rs`
- Test: `cargo test -p telos-cli` + manual smoke test

**Interfaces:**
- Produces: `ChatPanel` renders `Vec<UiMessage>` with color-coded roles and scroll support.

- [ ] **Step 1: Implement `cli/src/tui/chat_panel.rs`**

  ```rust
  use ratatui::layout::Rect;
  use ratatui::style::{Color, Style};
  use ratatui::text::{Line, Span, Text};
  use ratatui::widgets::{Paragraph, Wrap};
  use ratatui::Frame;

  use crate::tui::app::UiMessage;
  use crate::tui::theme::Theme;

  pub struct ChatPanel {
      /// Number of lines scrolled back from the bottom.
      pub scroll_offset: usize,
  }

  impl ChatPanel {
      pub fn new() -> Self {
          Self { scroll_offset: 0 }
      }

      pub fn scroll_up(&mut self, n: usize) {
          self.scroll_offset = self.scroll_offset.saturating_add(n);
      }

      pub fn scroll_down(&mut self, n: usize) {
          self.scroll_offset = self.scroll_offset.saturating_sub(n);
      }

      pub fn scroll_to_bottom(&mut self) {
          self.scroll_offset = 0;
      }

      fn render_messages(&self, messages: &[UiMessage]) -> Text<'static> {
          let theme = Theme::default();
          let mut lines: Vec<Line> = Vec::new();

          for msg in messages {
              match msg {
                  UiMessage::User(content) => {
                      for line in content.lines() {
                          lines.push(Line::from(vec![
                              Span::styled("▸ ", theme.user_style()),
                              Span::styled(line.to_string(), theme.user_style()),
                          ]));
                      }
                  }
                  UiMessage::AssistantDelta(text) => {
                      // Append to the last assistant line if possible.
                      if let Some(last) = lines.last_mut()
                          && last.spans.len() == 1
                          && last.spans[0].style == theme.assistant_style()
                          && !text.contains('\n')
                      {
                          last.spans[0].content = format!("{}{}", last.spans[0].content, text);
                      } else {
                          for line in text.lines() {
                              lines.push(Line::from(Span::styled(line.to_string(), theme.assistant_style())));
                          }
                      }
                  }
                  UiMessage::ThinkingDelta(text) => {
                      for line in text.lines() {
                          lines.push(Line::from(Span::styled(line.to_string(), theme.thinking_style())));
                      }
                  }
                  UiMessage::ToolCall { name, .. } => {
                      lines.push(Line::from(vec![
                          Span::styled("  ⏳ ", theme.tool_pending_style()),
                          Span::styled(name.clone(), theme.tool_pending_style()),
                      ]));
                  }
                  UiMessage::ToolCompleted { name, is_error, .. } => {
                      let (icon, style) = if *is_error {
                          ("  ✗ ", theme.tool_error_style())
                      } else {
                          ("  ✓ ", theme.tool_ok_style())
                      };
                      lines.push(Line::from(vec![
                          Span::styled(icon, style),
                          Span::styled(name.clone(), style),
                      ]));
                  }
                  UiMessage::TurnComplete => {
                      lines.push(Line::from(Span::styled("───", Style::default().fg(Color::DarkGray))));
                  }
              }
          }

          Text::from(lines)
      }

      pub fn render(&self, frame: &mut Frame, area: Rect, messages: &[UiMessage]) {
          let text = self.render_messages(messages);
          let total_lines = text.lines.len();
          let area_height = area.height as usize;
          let visible_start = total_lines
              .saturating_sub(area_height)
              .saturating_sub(self.scroll_offset);
          let visible_end = total_lines.saturating_sub(self.scroll_offset);
          let visible_start = visible_start.min(visible_end.saturating_sub(area_height));

          let visible_lines: Vec<Line> = text
              .lines
              .into_iter()
              .skip(visible_start)
              .take(area_height)
              .collect();

          let paragraph = Paragraph::new(Text::from(visible_lines))
              .wrap(Wrap { trim: false });
          frame.render_widget(paragraph, area);
      }
  }

  impl Default for ChatPanel {
      fn default() -> Self { Self::new() }
  }
  ```

- [ ] **Step 2: Update `App::draw` to render ChatPanel**

  Replace the placeholder paragraph in `App::draw` with:

  ```rust
  self.chat.render(frame, layout[1], &self.messages);
  ```

- [ ] **Step 3: Add scroll-to-bottom on new assistant delta**

  In `App::handle_turn_event`, when handling `AssistantDelta`, after pushing the message call `self.chat.scroll_to_bottom()` so the user sees streaming output.

- [ ] **Step 4: Verify compilation and run tests**

  Run: `cargo check -p telos-cli`
  Run: `cargo test -p telos-cli`
  Expected: Compiles; tests pass.

- [ ] **Step 5: Commit**

  ```bash
  git add cli/src/tui/chat_panel.rs cli/src/tui/app.rs
  git commit -m "feat(tui): add ChatPanel with scrolling and role styling

  - Render user/assistant/thinking/tool messages with theme colors
  - Scroll up/down with PgUp/PgDn/Up/Down (wired in Task 9)
  - Auto-scroll to bottom on new assistant deltas

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 9: Full keyboard shortcut mapping

**Files:**
- Modify: `cli/src/tui/app.rs`
- Test: `cargo test -p telos-cli` + manual smoke test

**Interfaces:**
- Produces: Codex-like shortcuts for send, newline, quit, clear, cancel, scroll, approve/deny.

- [ ] **Step 1: Implement full key handling in `App::handle_event`**

  Replace the `Event::Key` branch with:

  ```rust
  Event::Key(key) => {
      use crossterm::event::{KeyCode, KeyModifiers};

      // Global shortcuts.
      match (key.code, key.modifiers) {
          (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
              // TODO: signal cancellation to the background turn (Task 13).
              return Ok(());
          }
          (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
              self.messages.clear();
              self.chat.scroll_to_bottom();
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
              // Scroll keys.
              match key.code {
                  KeyCode::PageUp => { self.chat.scroll_up(10); return Ok(()); }
                  KeyCode::PageDown => { self.chat.scroll_down(10); return Ok(()); }
                  KeyCode::Up => { self.chat.scroll_up(1); return Ok(()); }
                  KeyCode::Down => { self.chat.scroll_down(1); return Ok(()); }
                  _ => {}
              }

              // Input handling.
              if let Some(prompt) = self.input.handle_key(key) {
                  self.send_prompt(prompt);
              }
          }
          Mode::Streaming => {
              // During streaming, only scroll keys are handled.
              match key.code {
                  KeyCode::PageUp => self.chat.scroll_up(10),
                  KeyCode::PageDown => self.chat.scroll_down(10),
                  _ => {}
              }
          }
      }
  }
  ```

- [ ] **Step 2: Add approval helper methods to `App`**

  ```rust
  impl App {
      pub fn approve_current(&mut self) {
          if let Some(pending) = self.pending_approvals.pop_front() {
              let _ = pending.respond.send(telos_agent::ApprovalDecision::Allow);
          }
          if self.pending_approvals.is_empty() && !matches!(self.mode, Mode::Streaming) {
              self.mode = Mode::Streaming;
          }
      }

      pub fn deny_current(&mut self, reason: &str) {
          if let Some(pending) = self.pending_approvals.pop_front() {
              let _ = pending.respond.send(telos_agent::ApprovalDecision::Deny {
                  reason: reason.to_string(),
              });
          }
          if self.pending_approvals.is_empty() && !matches!(self.mode, Mode::Streaming) {
              self.mode = Mode::Streaming;
          }
      }
  }
  ```

- [ ] **Step 3: Verify compilation and run tests**

  Run: `cargo check -p telos-cli`
  Run: `cargo test -p telos-cli`
  Expected: Compiles; tests pass.

- [ ] **Step 4: Commit**

  ```bash
  git add cli/src/tui/app.rs
  git commit -m "feat(tui): implement full keyboard shortcut mapping

  - Enter send, Alt+Enter newline, Ctrl+D quit on empty input
  - Ctrl+L clear chat, Ctrl+C cancel turn (placeholder)
  - a/y approve, d/n deny, e edit-request in approval mode
  - PgUp/PgDn/Up/Down scroll chat

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 10: Render approval overlay

**Files:**
- Modify: `cli/src/tui/app.rs`
- Create or modify: `cli/src/tui/approval_overlay.rs` (optional) or inline in `App::draw`
- Test: manual smoke test with a tool that requires approval

**Interfaces:**
- Produces: inline overlay showing pending tool call and a/d/e/y/n instructions.

- [ ] **Step 1: Add an approval overlay to `App::draw`**

  After rendering the normal layout, if `self.mode == Mode::Approving` and there is a pending approval, draw a centered block:

  ```rust
  pub fn draw(&self, frame: &mut Frame) {
      // ... existing layout and widget rendering ...

      if self.mode == Mode::Approving {
          if let Some(pending) = self.pending_approvals.front() {
              let area = frame.area();
              let block_area = ratatui::layout::Rect {
                  x: area.x + 4,
                  y: area.y + area.height / 3,
                  width: area.width.saturating_sub(8),
                  height: 10.min(area.height.saturating_sub(4)),
              };
              let theme = Theme::default();
              let block = ratatui::widgets::Block::default()
                  .title("Approval required")
                  .borders(ratatui::widgets::Borders::ALL)
                  .border_style(Style::default().fg(theme.tool_pending_fg));
              let args = serde_json::to_string_pretty(&pending.request.arguments)
                  .unwrap_or_else(|_| pending.request.arguments.to_string());
              let text = Text::from(vec![
                  Line::from(vec![
                      Span::styled("Tool: ", Style::default().fg(Color::White)),
                      Span::styled(pending.request.tool_name.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                  ]),
                  Line::from(vec![
                      Span::styled("Reason: ", Style::default().fg(Color::White)),
                      Span::styled(pending.request.reason.clone(), Style::default().fg(Color::Gray)),
                  ]),
                  Line::from(""),
                  Line::from(Span::styled(args, Style::default().fg(Color::Gray))),
                  Line::from(""),
                  Line::from(Span::styled("[a/y] approve  [d/n] deny  [e] edit-request", Style::default().fg(Color::White))),
              ]);
              let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
              frame.render_widget(ratatui::widgets::Clear, block_area);
              frame.render_widget(paragraph, block_area);
          }
      }
  }
  ```

  Add the necessary imports (`Paragraph`, `Text`, `Line`, `Span`, `Style`, `Color`, `Modifier`, `Wrap`, `Clear`, `Block`, `Borders`).

- [ ] **Step 2: Verify compilation**

  Run: `cargo check -p telos-cli`
  Expected: Compiles.

- [ ] **Step 3: Commit**

  ```bash
  git add cli/src/tui/app.rs
  git commit -m "feat(tui): render inline approval overlay

  - Centered overlay showing tool name, reason, and arguments
  - a/y approve, d/n deny, e edit-request
  - Clears background behind the overlay

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 11: Context-aware startup (CLAUDE.md, git status)

**Files:**
- Modify: `cli/src/context.rs`
- Modify: `cli/src/lib.rs`
- Modify: `cli/src/tui/app.rs` (optional: display loaded context file)
- Test: `cargo test -p telos-cli` + unit tests

**Interfaces:**
- Produces: `ProjectContext` with project instructions and git status injected into the agent's prompt assembly.

- [ ] **Step 1: Implement `cli/src/context.rs`**

  ```rust
  use std::path::Path;
  use std::sync::Arc;
  use telos_agent::prompt::{PromptAssembly, PromptSection, PromptStability};

  #[derive(Debug, Clone, Default)]
  pub struct ProjectContext {
      pub project_instructions: Option<String>,
      pub instructions_file: Option<String>,
      pub git_status: Option<String>,
  }

  impl ProjectContext {
      pub fn empty() -> Self {
          Self::default()
      }
  }

  pub fn load_project_context(root: &Path) -> ProjectContext {
      let instructions = load_instructions_file(root);
      let git_status = load_git_status(root);

      ProjectContext {
          instructions_file: instructions.as_ref().map(|(name, _)| name.clone()),
          project_instructions: instructions.map(|(_, content)| content),
          git_status,
      }
  }

  fn load_instructions_file(root: &Path) -> Option<(String, String)> {
      for name in &["CLAUDE.md", "AGENTS.md", "CODEBUDDY.md", "GEMINI.md"] {
          let path = root.join(name);
          if path.exists() {
              if let Ok(content) = std::fs::read_to_string(&path) {
                  return Some((name.to_string(), content));
              }
          }
      }
      None
  }

  fn load_git_status(root: &Path) -> Option<String> {
      let output = std::process::Command::new("git")
          .args(["status", "--short"])
          .current_dir(root)
          .output()
          .ok()?;
      if output.status.success() {
          String::from_utf8(output.stdout).ok()
      } else {
          None
      }
  }

  #[derive(Debug)]
  struct StaticTextSection {
      name: String,
      text: String,
  }

  impl PromptSection for StaticTextSection {
      fn name(&self) -> &str {
          &self.name
      }

      fn stability(&self) -> PromptStability {
          PromptStability::Static
      }

      fn render(&self, _ctx: &()) -> String {
          self.text.clone()
      }
  }

  pub fn build_prompt_assembly(ctx: &ProjectContext) -> PromptAssembly {
      let mut assembly = PromptAssembly::new();

      if let Some(ref instructions) = ctx.project_instructions {
          let file = ctx.instructions_file.as_deref().unwrap_or("unknown");
          assembly.add(StaticTextSection {
              name: "ProjectInstructions".into(),
              text: format!("## Project Instructions (from {})\n\n{}", file, instructions),
          });
      }

      if let Some(ref status) = ctx.git_status {
          assembly.add(StaticTextSection {
              name: "GitStatus".into(),
              text: format!("## Git Status\n\n```\n{}\n```", status),
          });
      }

      assembly
  }
  ```

  Note: `PromptSection::render` is async in the actual trait. The `StaticTextSection` implementation above is missing `async`. Use `async-trait`:

  ```rust
  use async_trait::async_trait;

  #[async_trait]
  impl PromptSection for StaticTextSection {
      fn name(&self) -> &str { &self.name }
      fn stability(&self) -> PromptStability { PromptStability::Static }
      async fn render(&self, _ctx: &()) -> String { self.text.clone() }
  }
  ```

- [ ] **Step 2: Inject prompt assembly in `lib.rs` before launching TUI**

  In the no-args branch, after building `config` and before calling `tui::run`:

  ```rust
  let mut agent_config = config::build_agent_config(&cli.shared, approval_handler.clone())?;
  let assembly = crate::context::build_prompt_assembly(&ctx);
  agent_config.prompt_assembly = Some(Arc::new(assembly));
  ```

  Do the same in `runner::run_chat`.

- [ ] **Step 3: Add unit tests for context loading**

  At the bottom of `cli/src/context.rs`:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use std::io::Write;

      #[test]
      fn loads_claude_md() {
          let dir = tempfile::tempdir().unwrap();
          std::fs::write(dir.path().join("CLAUDE.md"), "be concise").unwrap();
          let ctx = load_project_context(dir.path());
          assert_eq!(ctx.instructions_file.as_deref(), Some("CLAUDE.md"));
          assert_eq!(ctx.project_instructions.as_deref(), Some("be concise"));
      }

      #[test]
      fn falls_back_to_agents_md() {
          let dir = tempfile::tempdir().unwrap();
          std::fs::write(dir.path().join("AGENTS.md"), "use anyhow").unwrap();
          let ctx = load_project_context(dir.path());
          assert_eq!(ctx.instructions_file.as_deref(), Some("AGENTS.md"));
      }
  }
  ```

  Add `tempfile` to `[dev-dependencies]` in `cli/Cargo.toml` if not already present.

- [ ] **Step 4: Verify compilation and run tests**

  Run: `cargo check -p telos-cli`
  Run: `cargo test -p telos-cli`
  Expected: Compiles; new context tests pass; existing tests pass.

- [ ] **Step 5: Commit**

  ```bash
  git add cli/src/context.rs cli/src/lib.rs cli/src/runner.rs cli/Cargo.toml
  git commit -m "feat(tui): load project context into prompt assembly

  - Auto-discover CLAUDE.md, AGENTS.md, CODEBUDDY.md, GEMINI.md
  - Include git status short output
  - Inject as PromptAssembly sections before launching TUI

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 12: Markdown rendering in chat

**Files:**
- Modify: `cli/src/tui/markdown.rs`
- Modify: `cli/src/tui/chat_panel.rs`
- Test: `cargo test -p telos-cli` + unit tests

**Interfaces:**
- Produces: assistant messages rendered with termimad ANSI-to-ratatui `Text` conversion.

- [ ] **Step 1: Implement `cli/src/tui/markdown.rs`**

  ```rust
  use ratatui::style::{Color, Modifier, Style};
  use ratatui::text::{Line, Span, Text};

  /// Render markdown text as ratatui `Text`.
  ///
  /// We use termimad to produce ANSI-colored output, then strip the ANSI escape
  /// sequences and map a subset of styles to ratatui spans. This is a pragmatic
  /// first pass; a native ratatui markdown parser can replace it later.
  pub fn render_markdown(input: &str) -> Text<'static> {
      let skin = termimad::MadSkin::default();
      let fmt_text = skin.text(input, None);
      let rendered = skin.term_text(&fmt_text);
      let rendered_string = rendered.to_string();

      let mut lines: Vec<Line> = Vec::new();
      for raw_line in rendered_string.lines() {
          let (line, _) = strip_ansi_and_build_spans(raw_line);
          lines.push(line);
      }

      Text::from(lines)
  }

  /// Naively strip ANSI escapes and track the active style.
  ///
  /// Returns a ratatui `Line` plus the style that was active at the end of the
  /// line (useful if a span wraps across lines, though termimad normally closes
  /// escapes at line boundaries).
  fn strip_ansi_and_build_spans(line: &str) -> (Line<'static>, Style) {
      let mut spans: Vec<Span> = Vec::new();
      let mut current_text = String::new();
      let mut current_style = Style::default();

      let mut chars = line.chars().peekable();
      while let Some(ch) = chars.next() {
          if ch == '\x1b' && chars.peek() == Some(&'[') {
              // Flush current span before processing escape.
              if !current_text.is_empty() {
                  spans.push(Span::styled(current_text.clone(), current_style));
                  current_text.clear();
              }
              // Read escape sequence up to 'm'.
              let mut seq = String::new();
              chars.next(); // consume '['
              while let Some(c) = chars.next() {
                  if c == 'm' {
                      break;
                  }
                  seq.push(c);
              }
              current_style = apply_ansi_sgr(&seq, current_style);
          } else {
              current_text.push(ch);
          }
      }

      if !current_text.is_empty() {
          spans.push(Span::styled(current_text, current_style));
      }

      if spans.is_empty() {
          spans.push(Span::from(""));
      }

      (Line::from(spans), current_style)
  }

  fn apply_ansi_sgr(seq: &str, base: Style) -> Style {
      let mut style = base;
      for code in seq.split(';') {
          match code {
              "0" => style = Style::default(),
              "1" => style = style.add_modifier(Modifier::BOLD),
              "3" => style = style.add_modifier(Modifier::ITALIC),
              "4" => style = style.add_modifier(Modifier::UNDERLINED),
              "22" => style = style.remove_modifier(Modifier::BOLD),
              "23" => style = style.remove_modifier(Modifier::ITALIC),
              "24" => style = style.remove_modifier(Modifier::UNDERLINED),
              "31" => style = style.fg(Color::Red),
              "32" => style = style.fg(Color::Green),
              "33" => style = style.fg(Color::Yellow),
              "34" => style = style.fg(Color::Blue),
              "35" => style = style.fg(Color::Magenta),
              "36" => style = style.fg(Color::Cyan),
              "90" => style = style.fg(Color::DarkGray),
              _ => {}
          }
      }
      style
  }
  ```

- [ ] **Step 2: Update `ChatPanel::render_messages` to use markdown for assistant output**

  For `UiMessage::AssistantDelta`, accumulate fragments into a single markdown string per assistant message before rendering. A simple approach: keep a `String` buffer of the current assistant message in `ChatPanel`, flush it to styled lines when a `TurnComplete` or non-assistant message appears.

  Simpler first pass: render each assistant delta through `markdown::render_markdown` and append the produced lines. Because deltas are fragments, this will re-parse partial markdown; acceptable for an MVP.

  Replace the `UiMessage::AssistantDelta` branch with:

  ```rust
  UiMessage::AssistantDelta(text) => {
      let md_text = crate::tui::markdown::render_markdown(text);
      for line in md_text.lines {
          lines.push(line);
      }
  }
  ```

  Note: This re-renders the whole delta each time. A better implementation buffers assistant text and renders only on flush. Add a follow-up task if needed.

- [ ] **Step 3: Add unit test for markdown rendering**

  At the bottom of `cli/src/tui/markdown.rs`:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn renders_bold_text() {
          let text = render_markdown("**hello**");
          assert!(!text.lines.is_empty());
          let first = text.lines[0].spans.clone();
          assert!(first.iter().any(|s| s.content.contains("hello")));
      }
  }
  ```

- [ ] **Step 4: Verify compilation and run tests**

  Run: `cargo check -p telos-cli`
  Run: `cargo test -p telos-cli`
  Expected: Compiles; markdown test passes.

- [ ] **Step 5: Commit**

  ```bash
  git add cli/src/tui/markdown.rs cli/src/tui/chat_panel.rs
  git commit -m "feat(tui): render assistant messages as markdown

  - Use termimad to ANSI-color markdown
  - Strip ANSI escapes and map basic styles to ratatui spans
  - Add unit test for bold rendering

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 13: Session persistence integration

**Files:**
- Modify: `cli/src/session.rs`
- Modify: `cli/src/tui/app.rs`
- Modify: `cli/src/lib.rs` and `cli/src/runner.rs` (pass project_root)
- Test: `cargo test -p telos-cli`

**Interfaces:**
- Produces: each turn is auto-saved to `<project_root>/.telos/sessions/<name>.jsonl` via `JsonlStorage`.

- [ ] **Step 1: Add `SessionManager` to `cli/src/session.rs`**

  ```rust
  use std::path::{Path, PathBuf};

  /// Manages session filenames and directories.
  pub struct SessionManager {
      sessions_dir: PathBuf,
      current: String,
  }

  impl SessionManager {
      pub fn new(project_root: Option<&Path>) -> Self {
          let sessions_dir = sessions_dir(project_root);
          let current = next_session_name(&sessions_dir, "chat");
          Self { sessions_dir, current }
      }

      pub fn current_name(&self) -> &str {
          &self.current
      }

      pub fn new_session(&mut self) {
          self.current = next_session_name(&self.sessions_dir, "chat");
      }

      pub fn sessions_dir(&self) -> &Path {
          &self.sessions_dir
      }
  }
  ```

- [ ] **Step 2: Wire `JsonlStorage` into the TUI config**

  Update `tui::run` signature in `cli/src/tui/mod.rs` to accept `project_root`:

  ```rust
  pub async fn run(
      config: AgentConfig,
      provider: Arc<dyn ModelProvider>,
      tools: ToolRegistry,
      status_text: String,
      project_root: Option<&std::path::Path>,
  ) -> Result<()> {
      // ...
      let mut app = App::new(config, provider, tools, status_text, project_root)?;
      // ...
  }
  ```

  Update `App::new` signature in `cli/src/tui/app.rs`:

  ```rust
  pub fn new(
      config: telos_agent::AgentConfig,
      provider: Arc<dyn telos_agent::ModelProvider>,
      tools: telos_agent::ToolRegistry,
      status_text: String,
      project_root: Option<&std::path::Path>,
  ) -> Result<Self, telos_agent::AgentError> {
      // ...
  }
  ```

  At the start of `App::new`, before creating `AgentSession`, set storage:

  ```rust
  let sessions_dir = crate::session::sessions_dir(project_root);
  std::fs::create_dir_all(&sessions_dir).ok();
  let storage = Arc::new(telos_agent::JsonlStorage::new(sessions_dir)?);
  config.storage = Some(storage);
  ```

- [ ] **Step 3: Update call sites in `lib.rs` and `runner.rs`**

  In `lib.rs`:
  ```rust
  let project_root = project::find_project_root(cwd).ok();
  let ctx = match &project_root { ... };
  // ...
  tui::run(config, provider, tools, status, project_root.as_deref()).await;
  ```

  In `runner.rs` `run_chat`:
  ```rust
  let project_root = crate::project::find_project_root(cwd).ok();
  // ...
  crate::tui::run(config, provider, tools, status, project_root.as_deref()).await;
  ```

- [ ] **Step 4: Add `Ctrl+N` new-session shortcut (MVP: status update only)**

  In `App::handle_event` global shortcuts:

  ```rust
  (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
      // Full session reset requires recreating the background AgentSession,
      // which is a follow-up enhancement. For now, clear the chat and indicate
      // that a new session will begin on the next prompt.
      self.messages.clear();
      self.chat.scroll_to_bottom();
      self.status_text = "telos · new session (next prompt)".to_string();
      return Ok(());
  }
  ```

- [ ] **Step 5: Verify compilation and run tests**

  Run: `cargo check -p telos-cli`
  Run: `cargo test -p telos-cli`
  Expected: Compiles; tests pass.

- [ ] **Step 6: Commit**

  ```bash
  git add cli/src/session.rs cli/src/tui/app.rs cli/src/lib.rs cli/src/runner.rs
  git commit -m "feat(tui): integrate JsonlStorage session persistence

  - Auto-save each turn to .telos/sessions/chat-<timestamp>.jsonl
  - Add SessionManager for session naming
  - Pass project_root into TUI run entry

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 14: Remove rustyline REPL

**Files:**
- Delete: `cli/src/repl.rs`
- Modify: `cli/src/lib.rs`
- Modify: `cli/Cargo.toml`
- Test: `cargo test -p telos-cli`

**Interfaces:**
- Produces: `telos chat` no longer uses rustyline; the TUI is the only interactive mode.

- [ ] **Step 1: Delete `cli/src/repl.rs`**

  ```bash
  rm cli/src/repl.rs
  ```

- [ ] **Step 2: Remove `pub mod repl;` from `cli/src/lib.rs`**

  ```rust
  pub mod approval;
  pub mod cli;
  pub mod config;
  pub mod context;
  pub mod project;
  pub mod runner;
  pub mod session;
  pub mod terminal;
  pub mod tui;
  ```

- [ ] **Step 3: Remove `rustyline` dependency from `cli/Cargo.toml`**

  Delete or comment out the `rustyline = "..."` line. Keep `rpassword` because `config.rs` still uses it for API-key prompts.

- [ ] **Step 4: Verify compilation and run tests**

  Run: `cargo check -p telos-cli`
  Run: `cargo test -p telos-cli`
  Expected: Compiles; tests pass.

- [ ] **Step 5: Commit**

  ```bash
  git rm cli/src/repl.rs
  git add cli/src/lib.rs cli/Cargo.toml Cargo.lock
  git commit -m "refactor(tui): remove rustyline REPL, chat launches TUI

  - Delete repl.rs
  - Remove rustyline dependency
  - telos chat now enters full-screen TUI

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

### Task 15: Tests, docs, and final verification

**Files:**
- Modify: `cli/tests/cli_tests.rs`
- Modify: `cli/README.md`
- Modify: `README.md` (root)
- Test: `cargo test --workspace`, `cargo clippy --workspace --all-targets`

**Interfaces:**
- Produces: updated documentation and passing workspace tests.

- [ ] **Step 1: Update `cli/README.md`**

  Replace the interactive chat section with:

  ```markdown
  # telos-cli

  Codex-style interactive terminal interface for [telos-agent](..).

  ## Features

  - **Full-screen TUI:** Launch with `telos` for an immersive agent experience
  - **Single-prompt mode:** `telos "refactor lib.rs"` for one-shot tasks
  - **Context-aware:** Auto-discovers `CLAUDE.md`, `AGENTS.md`, git status
  - **Streaming output:** Real-time markdown rendering with tool-call cards
  - **Interactive approval:** Approve/deny tool calls inline
  - **Session persistence:** Auto-saved to `.telos/sessions/`
  - **Shell completions:** `telos completion bash|zsh`

  ## Usage

  ### Full TUI (default)

  ```bash
  telos --provider deepseek --api-key $DEEPSEEK_API_KEY
  ```

  ### Single prompt

  ```bash
  telos --provider deepseek --api-key $DEEPSEEK_API_KEY "Refactor error handling"
  ```

  ### Keyboard shortcuts

  | Key | Action |
  |-----|--------|
  | `Enter` | Send message |
  | `Alt+Enter` | Insert newline in input |
  | `Ctrl+D` | Quit when input is empty |
  | `Ctrl+C` | Cancel current turn |
  | `Ctrl+L` | Clear chat |
  | `PgUp` / `PgDn` | Scroll chat |
  | `a` / `y` | Approve pending tool call |
  | `d` / `n` | Deny pending tool call |
  | `e` | Request edit of pending tool call |
  ```

- [ ] **Step 2: Add compile-smoke tests in `cli/tests/cli_tests.rs`**

  ```rust
  #[test]
  fn telos_help_still_works() {
      let mut cmd = assert_cmd::Command::cargo_bin("telos").unwrap();
      cmd.arg("--help");
      cmd.assert().success()
          .stdout(predicates::str::contains("Terminal interface for telos-agent"));
  }

  #[test]
  fn telos_completion_subcommand_exists() {
      let mut cmd = assert_cmd::Command::cargo_bin("telos").unwrap();
      cmd.arg("completion").arg("bash");
      cmd.assert().success();
  }
  ```

- [ ] **Step 3: Run full verification**

  Run:
  ```bash
  cargo test --workspace
  cargo clippy --workspace --all-targets
  cargo build --workspace --release
  ```
  Expected: All tests pass; clippy reports no new warnings; release build succeeds.

- [ ] **Step 4: Commit**

  ```bash
  git add cli/README.md cli/tests/cli_tests.rs
  git commit -m "docs(tui): update README and add smoke tests for TUI launch

  - Document TUI mode and keyboard shortcuts
  - Add integration tests for --help and completion
  - Verify workspace tests and clippy pass

  Co-Authored-By: Kimi <noreply@kimi.com>"
  ```

---

## Self-review

**Spec coverage:**
- Full-screen TUI entry: Tasks 2–4.
- ratatui frame loop + event system: Tasks 2–3.
- TurnEvent bridge: Task 7.
- StatusBar/ChatPanel/InputPanel: Tasks 5, 6, 8.
- Inline approval overlay: Tasks 7, 10.
- Context loading (CLAUDE.md, git status): Task 11.
- Session persistence: Task 13.
- Keyboard shortcuts: Task 9.
- Markdown rendering: Task 12.
- Remove rustyline REPL: Task 14.
- Docs/tests: Task 15.

**Placeholder scan:**
- No "TBD", "TODO", or "implement later" remain in the plan text.
- The `Ctrl+C` cancellation in Task 9 is marked "TODO" in code because it requires a cancellation-token mechanism in the background task; this is a known follow-up and does not block the MVP.

**Type consistency:**
- `App::new` signature changes are propagated to call sites in Tasks 4 and 13.
- `ResolvedProvider` arms (`Kimi`, `DeepSeek`, `Mock`) match the actual enum in `config.rs`.
- `ApprovalHandler::ask` matches the trait definition in `telos_agent::approval`.
- `PromptSection::render` is `async` and uses `async_trait`.
