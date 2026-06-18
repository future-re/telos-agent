# Prompt System v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add tool-specific prompts, system-reminder lifecycle messages, a static/dynamic prompt cache boundary, and bundled Superpowers/deep-search skills to telos-agent.

**Architecture:** Extend the existing `Tool` trait with an optional `prompt_text()` method, render collected prompts via a new `ToolPromptsSection`, introduce a `SystemReminder` enum injected by `AgentSession`, split `PromptAssembly` output into `PromptBlock`s with stability metadata, and bundle skill markdowns loaded at startup.

**Tech Stack:** Rust, async-trait, serde_json, tokio, cargo

---

## File map

| File | Responsibility |
|------|----------------|
| `src/tool/mod.rs` | `Tool` trait, `ToolRegistry`, new `prompt_text()` and `iter()` APIs |
| `src/tools/shell.rs` | `ShellTool::prompt_text()` for `Bash` |
| `src/tools/file_read.rs` | `FileReadTool::prompt_text()` for `Read` |
| `src/tools/file_edit.rs` | `FileEditTool::prompt_text()` for `Edit` |
| `src/tools/file_write.rs` | `FileWriteTool::prompt_text()` for `Write` |
| `src/tools/glob.rs` | `GlobTool::prompt_text()` for `Glob` |
| `src/tools/grep.rs` | `GrepTool::prompt_text()` for `Grep` |
| `src/tools/skill.rs` | `SkillTool::prompt_text()` for `Skill` |
| `src/tools/ask_user_question.rs` | `AskUserQuestionTool::prompt_text()` |
| `src/tools/web_search.rs` | `WebSearchTool::prompt_text()` |
| `src/tools/web_fetch.rs` | `WebFetchTool::prompt_text()` |
| `src/subagent/mod.rs` | `SubagentTool::prompt_text()` for `Subagent` |
| `src/prompt/section.rs` | `PromptBlock`, `CacheHint` (re-exported) |
| `src/prompt/assembly.rs` | `build_blocks()` method |
| `src/prompt/builtins.rs` | `ToolPromptsSection`, bundled-skill section wiring |
| `src/prompt/mod.rs` | Re-exports, `default_coding_assembly` update |
| `src/provider/types.rs` | `CompletionRequest` gains `system_prompt_blocks` |
| `src/provider/openai_compat.rs` | Fall back / join blocks when present |
| `src/message.rs` | `SystemReminder` enum and rendering |
| `src/runtime.rs` | Inject reminders at lifecycle points |
| `src/config.rs` | `with_bundled_skills()` helper |
| `src/skills/registry.rs` | `load_bundled_skills()` |
| `src/skills/bundled/explore.md` | New bundled deep-search skill |
| `tests/integration_tests.rs` | New tests for prompts, reminders, blocks, bundled skills |

---

### Task 1: Extend `Tool` trait with `prompt_text()`

**Files:**
- Modify: `src/tool/mod.rs:134-186`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write the failing test**

Add a test that asserts a custom tool can override `prompt_text()` and the default returns `None`:

```rust
#[test]
fn tool_prompt_text_defaults_to_none() {
    struct NoPromptTool;
    #[async_trait::async_trait]
    impl Tool for NoPromptTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "no_prompt".into(),
                description: "x".into(),
                input_schema: serde_json::json!({ "type": "object" }),
            }
        }
        async fn invoke(&self, _args: Value, _ctx: ToolContext) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }
    assert!(NoPromptTool.prompt_text().is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet tool_prompt_text_defaults_to_none`
Expected: compile error — method `prompt_text` not found

- [ ] **Step 3: Add the method to the trait**

In `src/tool/mod.rs` inside `pub trait Tool: Send + Sync`, after `fn definition(&self) -> ToolDefinition;`:

```rust
    /// Optional detailed usage instructions injected into the system prompt.
    /// Return `None` if the tool has no extra behavioral guidance.
    fn prompt_text(&self) -> Option<&'static str> {
        None
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --quiet tool_prompt_text_defaults_to_none`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/tool/mod.rs tests/integration_tests.rs
git commit -m "feat(tool): add optional prompt_text() trait method"
```

---

### Task 2: Add `ToolRegistry::iter()`

**Files:**
- Modify: `src/tool/mod.rs:199-389`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn tool_registry_iterates_tools() {
    let mut registry = ToolRegistry::new();
    registry.register(AddTool);
    let names: Vec<_> = registry.iter().map(|(n, _)| n.clone()).collect();
    assert!(names.contains(&"add".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet tool_registry_iterates_tools`
Expected: compile error — no method `iter` on `ToolRegistry`

- [ ] **Step 3: Implement `iter()`**

In `src/tool/mod.rs` inside `impl ToolRegistry`, add:

```rust
    /// Iterate all registered tools as `(canonical_name, tool)` pairs.
    /// The `Arc` is cloned; the underlying tool is shared.
    pub fn iter(&self) -> impl Iterator<Item = (&String, Arc<dyn Tool>)> + '_ {
        self.tools.iter().map(|(name, tool)| (name, Arc::clone(tool)))
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --quiet tool_registry_iterates_tools`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/tool/mod.rs tests/integration_tests.rs
git commit -m "feat(tool): add ToolRegistry::iter()"
```

---

### Task 3: Add `ToolPromptsSection`

**Files:**
- Modify: `src/prompt/builtins.rs`
- Modify: `src/prompt/mod.rs`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn tool_prompts_section_renders_registered_prompts() {
    use telos_agent::prompt::{PromptSection, PromptStability};
    use telos_agent::prompt::builtins::ToolPromptsSection;
    use telos_agent::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
    use async_trait::async_trait;

    struct PromptedTool;
    #[async_trait]
    impl Tool for PromptedTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "prompted".into(),
                description: "d".into(),
                input_schema: serde_json::json!({ "type": "object" }),
            }
        }
        fn prompt_text(&self) -> Option<&'static str> {
            Some("Always run this tool first.")
        }
        async fn invoke(&self, _args: Value, _ctx: ToolContext) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }

    let mut registry = ToolRegistry::new();
    registry.register(PromptedTool);
    let section = ToolPromptsSection::new(Arc::new(registry));
    let text = section.render(&()).await;
    assert!(text.contains("## Tool-specific guidance"));
    assert!(text.contains("prompted"));
    assert!(text.contains("Always run this tool first."));
    assert_eq!(section.stability(), PromptStability::Static);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet tool_prompts_section_renders_registered_prompts`
Expected: compile error — `ToolPromptsSection` not found

- [ ] **Step 3: Implement `ToolPromptsSection`**

In `src/prompt/builtins.rs`, add after the `ToolsSection` impl:

```rust
// ── Tool Prompts ──────────────────────────────────────────

/// Per-tool behavioral guidance collected from `Tool::prompt_text()`.
pub struct ToolPromptsSection {
    tools: Arc<ToolRegistry>,
}

impl ToolPromptsSection {
    pub fn new(tools: Arc<ToolRegistry>) -> Self {
        Self { tools }
    }
}

#[async_trait]
impl PromptSection for ToolPromptsSection {
    fn name(&self) -> &str {
        "tool_prompts"
    }
    fn stability(&self) -> PromptStability {
        PromptStability::Static
    }

    async fn render(&self, _ctx: &()) -> String {
        let mut entries: Vec<(String, String)> = self
            .tools
            .iter()
            .filter_map(|(name, tool)| {
                tool.prompt_text().map(|text| (name.clone(), text.to_string()))
            })
            .collect();
        if entries.is_empty() {
            return String::new();
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut lines = vec!["## Tool-specific guidance".to_string()];
        for (name, text) in entries {
            lines.push(format!("### {name}"));
            for line in text.lines() {
                lines.push(line.to_string());
            }
        }
        lines.join("\n")
    }
}
```

- [ ] **Step 4: Export from `src/prompt/mod.rs`**

Add `ToolPromptsSection` to the `pub use builtins::{...}` list.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --quiet tool_prompts_section_renders_registered_prompts`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/prompt/builtins.rs src/prompt/mod.rs tests/integration_tests.rs
git commit -m "feat(prompt): add ToolPromptsSection"
```

---

### Task 4: Add tool prompts to all core tools

**Files:**
- Modify: `src/tools/shell.rs`, `src/tools/file_read.rs`, `src/tools/file_edit.rs`, `src/tools/file_write.rs`, `src/tools/glob.rs`, `src/tools/grep.rs`, `src/tools/skill.rs`, `src/tools/ask_user_question.rs`, `src/tools/web_search.rs`, `src/tools/web_fetch.rs`, `src/subagent/mod.rs`

For each tool, add a `fn prompt_text(&self) -> Option<&'static str>` implementation returning adapted guidance from `Piebald-AI/claude-code-system-prompts`. Keep each prompt focused and concise (≈10–30 lines).

- [ ] **Step 1: `ShellTool` prompt**

In `src/tools/shell.rs`, inside `impl Tool for ShellTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use the Bash tool for shell commands, build/test runners, git operations, and other tasks that genuinely require a shell. \
Prefer Read, Edit, Write, Glob, or Grep for file operations. \
Provide a short `description` summarizing the command's intent. \
Commands run with a clean environment; pass required env vars explicitly. \
Avoid commands that require superuser privileges unless explicitly instructed.",
        )
    }
```

- [ ] **Step 2: `FileReadTool` prompt**

In `src/tools/file_read.rs`, inside `impl Tool for FileReadTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use Read to inspect UTF-8 text files. Always Read a file before editing or overwriting it. \
Use `offset`/`limit` to read large files in chunks. The returned content prefixes lines with 1-indexed line numbers; \
provide the original file text (without line-number prefixes) to Edit.",
        )
    }
```

- [ ] **Step 3: `FileEditTool` prompt**

In `src/tools/file_edit.rs`, inside `impl Tool for FileEditTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use Edit to make precise, exact-match replacements in a UTF-8 text file. The file must have been Read first. \
`old_string` must match exactly once unless `replace_all` is true. Include enough surrounding context to make `old_string` unique. \
Use an empty `old_string` only to create a new file. Do not use Edit on binary files or Jupyter notebooks.",
        )
    }
```

- [ ] **Step 4: `FileWriteTool` prompt**

In `src/tools/file_write.rs`, inside `impl Tool for FileWriteTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use Write to create a new file or overwrite an existing UTF-8 text file. \
If the file already exists, Read it first. Prefer Edit for small changes to existing files. \
Create parent directories automatically.",
        )
    }
```

- [ ] **Step 5: `GlobTool` prompt**

In `src/tools/glob.rs`, inside `impl Tool for GlobTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use Glob to list files matching a pattern under the working directory. \
Patterns are relative to cwd; absolute patterns are rejected. Use a literal extension or subdirectory anchor (e.g. `src/**/*.rs`).",
        )
    }
```

- [ ] **Step 6: `GrepTool` prompt**

In `src/tools/grep.rs`, inside `impl Tool for GrepTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use Grep to search UTF-8 files for a literal text pattern. Searches are substring matches, not regex. \
Results include path, 1-indexed line number, and matched line. Use `glob` to scope the search; default is `**/*`.",
        )
    }
```

- [ ] **Step 7: `SkillTool` prompt**

In `src/tools/skill.rs`, inside `impl Tool for SkillTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use the Skill tool to invoke loaded skills by name. Only invoke skills listed as available; do not guess. \
Pass `args` when the skill expects arguments. The skill returns its prompt and body for you to follow.",
        )
    }
```

- [ ] **Step 8: `AskUserQuestionTool` prompt**

In `src/tools/ask_user_question.rs`, inside `impl Tool for AskUserQuestionTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use AskUserQuestion to collect user preferences or disambiguate requirements when multiple valid choices exist. \
Provide 2-4 concrete options with concise descriptions. Do not ask questions you can infer from context or project conventions.",
        )
    }
```

- [ ] **Step 9: `WebSearchTool` prompt**

In `src/tools/web_search.rs`, inside `impl Tool for WebSearchTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use WebSearch when you need up-to-date information not present in the codebase or conversation. \
Summarize findings and cite sources. Prefer WebFetch when you already know the exact URL.",
        )
    }
```

- [ ] **Step 10: `WebFetchTool` prompt**

In `src/tools/web_fetch.rs`, inside `impl Tool for WebFetchTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use WebFetch to retrieve a specific public URL. Only fetch public `http`/`https` URLs. \
Verify that returned content is relevant and trustworthy before acting on it.",
        )
    }
```

- [ ] **Step 11: `SubagentTool` prompt**

In `src/subagent/mod.rs`, inside `impl Tool for SubagentTool` after `fn definition()`:

```rust
    fn prompt_text(&self) -> Option<&'static str> {
        Some(
            "Use the Subagent tool to delegate self-contained tasks, run parallel explore lenses, or protect the main context window. \
Provide a clear prompt and optional system_prompt. Do not duplicate work already being performed in the parent session.",
        )
    }
```

- [ ] **Step 12: Run tests**

Run: `cargo test --quiet`
Expected: all existing tests still pass

- [ ] **Step 13: Commit**

```bash
git add src/tools/*.rs src/subagent/mod.rs
git commit -m "feat(tools): add prompt_text() guidance to core tools"
```

---

### Task 5: Wire `ToolPromptsSection` into default assembly

**Files:**
- Modify: `src/prompt/mod.rs:33-47`
- Modify: `src/config.rs`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Update `default_coding_assembly`**

Change `src/prompt/mod.rs`:

```rust
pub fn default_coding_assembly(
    tools: Arc<ToolRegistry>,
    cwd: PathBuf,
) -> PromptAssembly {
    let mut assembly = PromptAssembly::new();
    assembly.add_static(IdentitySection::new(None));
    assembly.add_static(ToneStyleSection);
    assembly.add_static(TaskGuidanceSection);
    assembly.add_static(SafetySection);
    assembly.add_static(ToolUsageSection);
    assembly.add_static(ToolsSection::new(Arc::clone(&tools)));
    assembly.add_static(ToolPromptsSection::new(Arc::clone(&tools)));
    assembly.add_dynamic(DateSection);
    assembly.add_dynamic(CwdSection::new(cwd));
    assembly
}
```

- [ ] **Step 2: Write an integration test**

Add to `tests/integration_tests.rs`:

```rust
#[test]
fn default_assembly_includes_tool_prompts() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut tools = ToolRegistry::new();
        register_core_tools(&mut tools);
        let assembly = telos_agent::prompt::default_coding_assembly(
            Arc::new(tools),
            std::env::current_dir().unwrap(),
        );
        let text = assembly.build().await;
        assert!(text.contains("## Tool-specific guidance"));
        assert!(text.contains("### Bash"));
        assert!(text.contains("### Read"));
    });
}
```

- [ ] **Step 3: Run test**

Run: `cargo test --quiet default_assembly_includes_tool_prompts`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/prompt/mod.rs src/config.rs tests/integration_tests.rs
git commit -m "feat(prompt): wire ToolPromptsSection into default assembly"
```

---

### Task 6: Introduce `SystemReminder` and rendering

**Files:**
- Modify: `src/message.rs`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn system_reminder_renders_with_tags() {
    use telos_agent::message::SystemReminder;
    let reminder = SystemReminder::Compaction { reason: "token_budget".into() };
    let text = reminder.render();
    assert!(text.contains("<system-reminder>"));
    assert!(text.contains("token_budget"));
    assert!(text.contains("</system-reminder>"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet system_reminder_renders_with_tags`
Expected: compile error — `SystemReminder` not found

- [ ] **Step 3: Add the enum and rendering**

In `src/message.rs`, insert before the `#[cfg(test)]` module near the end of the file:

```rust
/// A system-level note injected into the conversation as a user message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemReminder {
    /// Entering plan mode; instructions may be restricted.
    PlanMode,
    /// Conversation was compacted; some prior context may have been summarized.
    Compaction { reason: String },
    /// Provider or model context changed mid-session.
    ProviderContext,
    /// A hook intercepted the assistant output and may have modified it.
    HookInterception { phase: String, name: String },
    /// A tool result contains a system-level note.
    ToolResult { tool_name: String, note: String },
}

impl SystemReminder {
    /// Render the reminder as a `<system-reminder>` XML block.
    pub fn render(&self) -> String {
        let body = match self {
            SystemReminder::PlanMode => {
                "You are entering plan mode. Follow the plan instructions and do not write implementation code until the plan is approved.".to_string()
            }
            SystemReminder::Compaction { reason } => {
                format!("Prior messages were compacted (reason: {reason}). Some context may have been summarized.")
            }
            SystemReminder::ProviderContext => {
                "The provider/model context has changed. Adjust to any new instructions or constraints.".to_string()
            }
            SystemReminder::HookInterception { phase, name } => {
                format!("A hook intercepted this turn during the {phase} phase ({name}). Treat hook output as user feedback.")
            }
            SystemReminder::ToolResult { tool_name, note } => {
                format!("Tool `{tool_name}` reported: {note}")
            }
        };
        format!("<system-reminder>\n{}\n</system-reminder>", body)
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --quiet system_reminder_renders_with_tags`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/message.rs tests/integration_tests.rs
git commit -m "feat(message): add SystemReminder enum and rendering"
```

---

### Task 7: Inject compaction and hook reminders in runtime

**Files:**
- Modify: `src/runtime.rs:272-322` and `src/runtime.rs:444-474`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Add helper to push reminders**

In `src/runtime.rs` inside `impl AgentSession`, add:

```rust
    fn push_system_reminder(&mut self, reminder: crate::message::SystemReminder) {
        self.messages.push(crate::message::Message::user(reminder.render()));
    }
```

- [ ] **Step 2: Inject after compaction**

In `run_compaction_phase`, after a compaction actually modifies messages (`compactions += 1` for token budget or char budget), add:

```rust
if did_compact {
    compactions += 1;
    self.push_system_reminder(crate::message::SystemReminder::Compaction {
        reason: "token_budget".into(),
    });
}
```

Do the same for the char-budget branch with `reason: "char_budget"`.

- [ ] **Step 3: Inject after hook emits a message**

In `run_hook_phase`, after pushing the emitted `Assistant` message, add:

```rust
if emitted {
    self.push_system_reminder(crate::message::SystemReminder::HookInterception {
        phase: phase_name.clone(),
        name: hook.name().to_string(),
    });
}
```

- [ ] **Step 4: Write integration test**

```rust
#[test]
fn compaction_emits_system_reminder() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let provider = MockProvider::new(vec![
            CompletionResponse {
                message: Message::assistant("hi"),
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
        let tools = ToolRegistry::new();
        let mut session = AgentSession::new(AgentConfig {
            token_budget: Some(TokenBudget {
                max_tokens: 1_000_000,
                compact_at_tokens: 1,
            }),
            compaction: Some(Arc::new(SummaryCompaction::default())),
            ..AgentConfig::default()
        }).unwrap();

        let _ = session.run_turn(&provider, &tools, "hello").await.unwrap();
        let has_reminder = session.messages().iter().any(|m| {
            m.role == telos_agent::Role::User && m.text_content().contains("<system-reminder>")
        });
        assert!(has_reminder);
    });
}
```

- [ ] **Step 5: Run test**

Run: `cargo test --quiet compaction_emits_system_reminder`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/runtime.rs tests/integration_tests.rs
git commit -m "feat(runtime): inject system reminders on compaction and hooks"
```

---

### Task 8: Add `PromptBlock` and `build_blocks()` to prompt assembly

**Files:**
- Modify: `src/prompt/section.rs`
- Modify: `src/prompt/assembly.rs`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn prompt_assembly_build_blocks_preserves_stability() {
    use telos_agent::prompt::{PromptAssembly, PromptSection, PromptStability, PromptBlock};

    struct StaticSection;
    #[async_trait::async_trait]
    impl PromptSection for StaticSection {
        fn name(&self) -> &str { "static" }
        fn stability(&self) -> PromptStability { PromptStability::Static }
        async fn render(&self, _ctx: &()) -> String { "static text".into() }
    }

    struct DynamicSection;
    #[async_trait::async_trait]
    impl PromptSection for DynamicSection {
        fn name(&self) -> &str { "dynamic" }
        fn stability(&self) -> PromptStability { PromptStability::Dynamic }
        async fn render(&self, _ctx: &()) -> String { "dynamic text".into() }
    }

    let mut assembly = PromptAssembly::new();
    assembly.add_static(StaticSection);
    assembly.add_dynamic(DynamicSection);
    let blocks = assembly.build_blocks().await;
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].name, "static");
    assert_eq!(blocks[0].stability, PromptStability::Static);
    assert_eq!(blocks[1].name, "dynamic");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet prompt_assembly_build_blocks_preserves_stability`
Expected: compile error — `PromptBlock`, `build_blocks` not found

- [ ] **Step 3: Add `PromptBlock`**

In `src/prompt/section.rs`:

```rust
/// A rendered prompt section with caching metadata.
#[derive(Debug, Clone)]
pub struct PromptBlock {
    pub name: String,
    pub text: String,
    pub stability: PromptStability,
}

/// Hint to providers about whether a block should be cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHint {
    Static,
    Dynamic,
}

impl From<PromptStability> for CacheHint {
    fn from(value: PromptStability) -> Self {
        match value {
            PromptStability::Static => CacheHint::Static,
            PromptStability::Dynamic => CacheHint::Dynamic,
        }
    }
}
```

- [ ] **Step 4: Implement `build_blocks()`**

In `src/prompt/assembly.rs`:

```rust
use crate::prompt::section::{PromptBlock, PromptStability};

impl PromptAssembly {
    pub async fn build_blocks(&self) -> Vec<PromptBlock> {
        let mut blocks = Vec::new();
        for section in &self.sections {
            let text = match section.stability() {
                PromptStability::Static => {
                    let mut cache = self.static_cache.lock().await;
                    if let Some(cached) = cache.get(section.name()) {
                        cached.clone()
                    } else {
                        let rendered = section.render(&()).await;
                        cache.insert(section.name().to_string(), rendered.clone());
                        rendered
                    }
                }
                PromptStability::Dynamic => section.render(&()).await,
            };
            if !text.is_empty() {
                blocks.push(PromptBlock {
                    name: section.name().to_string(),
                    text,
                    stability: section.stability(),
                });
            }
        }
        blocks
    }
}
```

- [ ] **Step 5: Re-export `PromptBlock` and `CacheHint`**

In `src/prompt/mod.rs`, add them to `pub use section::{...}`.

- [ ] **Step 6: Run test**

Run: `cargo test --quiet prompt_assembly_build_blocks_preserves_stability`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/prompt/section.rs src/prompt/assembly.rs src/prompt/mod.rs tests/integration_tests.rs
git commit -m "feat(prompt): add PromptBlock and build_blocks() for cache boundary"
```

---

### Task 9: Extend `CompletionRequest` with optional blocks

**Files:**
- Modify: `src/provider/types.rs`
- Modify: `src/provider/openai_compat.rs:82-111`
- Modify: `src/runtime.rs:343-353`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Update `CompletionRequest`**

In `src/provider/types.rs`:

```rust
use crate::prompt::PromptBlock;

pub struct CompletionRequest {
    pub system_prompt: Option<String>,
    /// Optional structured system prompt blocks for providers that support
    /// per-block cache control (e.g., Anthropic prompt caching).
    pub system_prompt_blocks: Option<Vec<PromptBlock>>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
}
```

- [ ] **Step 2: Update `openai_compat::build_request`**

Change the system-prompt insertion logic:

```rust
    let system_prompt_text = request
        .system_prompt
        .or_else(|| {
            request.system_prompt_blocks.map(|blocks| {
                blocks
                    .into_iter()
                    .map(|b| b.text)
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
        });

    if let Some(system_prompt) = system_prompt_text {
        let already_has_system =
            matches!(messages.first(), Some(ChatCompletionRequestMessage::System(_)));
        if !already_has_system {
            messages.insert(
                0,
                ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                    content: ChatCompletionRequestSystemMessageContent::Text(system_prompt),
                    name: None,
                }),
            );
        }
    }
```

- [ ] **Step 3: Update runtime to use blocks**

In `src/runtime.rs` `call_provider`:

```rust
            let (system_prompt, system_prompt_blocks) = if let Some(assembly) = &self.config.prompt_assembly {
                let blocks = assembly.build_blocks().await;
                (None, Some(blocks))
            } else {
                (self.config.base_system_prompt.clone(), None)
            };

            let request = CompletionRequest {
                system_prompt,
                system_prompt_blocks,
                messages: self.messages.clone(),
                tools: tool_definitions.to_vec(),
            };
```

- [ ] **Step 4: Fix all `CompletionRequest` struct literals**

Find all places that construct `CompletionRequest` with struct literal (tests, providers) and add `system_prompt_blocks: None,` or use `..Default::default()` after deriving `Default`. Search with:

```bash
grep -R "CompletionRequest {" src tests examples
```

Add `system_prompt_blocks: None,` to each.

- [ ] **Step 5: Run tests**

Run: `cargo test --quiet`
Expected: all tests pass; clippy clean

- [ ] **Step 6: Commit**

```bash
git add src/provider/types.rs src/provider/openai_compat.rs src/runtime.rs tests/integration_tests.rs src/provider/*.rs examples/kimi_tool_loop.rs
git commit -m "feat(provider): expose prompt cache boundary via system_prompt_blocks"
```

---

### Task 10: Add the bundled explore skill

The crate already bundles skills in `src/skills/bundled/` (`verify`, `debug`, `remember`, `brainstorm`, `update-config`) via `SkillLoader::load_bundled_skills()`. We add a deep-search / explore skill there.

**Files:**
- Create: `src/skills/bundled/explore.md`
- Modify: `src/skills/loader.rs` (register the new bundled file)
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Create `src/skills/bundled/explore.md`**

Use the existing frontmatter schema (`whenToUse`, not `when_to_use`; `prompt` is required):

```markdown
---
name: explore
description: Deep codebase exploration and research.
whenToUse: |
  Use when you need to understand a large or unfamiliar codebase, find where a feature lives,
  or research cross-cutting concerns before making changes.
prompt: |
  You are an explore agent. Your job is to research the codebase and report findings concisely.

  - Ask clarifying questions only when the user's request is genuinely ambiguous.
  - Start with targeted searches (Glob/Grep) for known identifiers or file patterns.
  - For broad cross-cutting research, delegate parallel searches via the Subagent tool with subagent_type Explore.
  - Read files to confirm assumptions; do not modify code during exploration.
  - Cite findings with `path/to/file.rs:line`.
  - Summarize: what you found, where it lives, and any recommended next steps.
---
```

- [ ] **Step 2: Register the new bundled file**

In `src/skills/loader.rs`, add `("explore.md", include_str!("bundled/explore.md"))` to the `files` array in `load_bundled_skills()`.

- [ ] **Step 3: Update the existing bundled-skills test**

The existing test `bundled_skills_load_successfully` already asserts `skills.len() >= 5`. Update it to `>= 6`:

```rust
assert!(skills.len() >= 6, "expected >=6 bundled skills, got {}", skills.len());
```

- [ ] **Step 4: Run tests**

Run: `cargo test --quiet bundled_skills_load_successfully`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/skills/bundled/explore.md src/skills/loader.rs tests/integration_tests.rs
git commit -m "feat(skills): add bundled explore skill"
```

---

### Task 11: Wire bundled skills into the default assembly

**Files:**
- Modify: `src/skills/registry.rs`
- Modify: `src/config.rs`
- Modify: `src/tools/mod.rs`
- Modify: `src/prompt/mod.rs`
- Modify: `src/runtime.rs`
- Modify: `src/lib.rs` (exports if needed)
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Add `SkillRegistry::load_bundled_skills()`**

In `src/skills/registry.rs`:

```rust
impl SkillRegistry {
    /// Load all skills bundled with the crate and register them.
    pub fn load_bundled_skills(&mut self) {
        for skill in crate::skills::loader::SkillLoader::load_bundled_skills() {
            self.register(skill);
        }
    }
}
```

- [ ] **Step 2: Add `skill_registry` to `AgentConfig`**

In `src/config.rs`, add to the `AgentConfig` struct:

```rust
    /// Optional skill registry. When set, the Skill tool and the default prompt
    /// assembly can use registered skills (including bundled Superpowers skills).
    pub skill_registry: Option<Arc<crate::skills::SkillRegistry>>,
```

Initialize it to `None` in `Default::default()`.

- [ ] **Step 3: Add `AgentConfig::with_bundled_skills()`**

In `src/config.rs`:

```rust
impl AgentConfig {
    /// Load bundled skills into a fresh skill registry attached to this config.
    pub fn with_bundled_skills(mut self) -> Self {
        let mut registry = crate::skills::SkillRegistry::new();
        registry.load_bundled_skills();
        self.skill_registry = Some(Arc::new(registry));
        self
    }
}
```

- [ ] **Step 4: Add `register_skill_tool`**

In `src/tools/mod.rs`, add after `register_task_tools`:

```rust
/// Register the Skill tool if a skill registry is available.
pub fn register_skill_tool(registry: &mut ToolRegistry, skill_registry: Arc<SkillRegistry>) {
    registry.register(SkillTool::new(skill_registry));
}
```

- [ ] **Step 5: Update `default_coding_assembly` signature**

In `src/prompt/mod.rs`:

```rust
pub fn default_coding_assembly(
    tools: Arc<ToolRegistry>,
    cwd: PathBuf,
    skills: Option<Arc<crate::skills::SkillRegistry>>,
) -> PromptAssembly {
    let mut assembly = PromptAssembly::new();
    assembly.add_static(IdentitySection::new(None));
    assembly.add_static(ToneStyleSection);
    assembly.add_static(TaskGuidanceSection);
    assembly.add_static(SafetySection);
    assembly.add_static(ToolUsageSection);
    assembly.add_static(ToolsSection::new(Arc::clone(&tools)));
    assembly.add_static(ToolPromptsSection::new(Arc::clone(&tools)));
    assembly.add_dynamic(DateSection);
    assembly.add_dynamic(CwdSection::new(cwd));
    if let Some(skills) = skills {
        assembly.add_dynamic(crate::prompt::builtins::SkillsSection::new(skills));
    }
    assembly
}
```

- [ ] **Step 6: Update callers of `default_coding_assembly`**

Find with:

```bash
grep -R "default_coding_assembly" src tests examples
```

Update each call site to pass the third argument (`None` or `config.skill_registry.clone()`). In `src/config.rs`, update `with_default_prompt_assembly`:

```rust
pub fn with_default_prompt_assembly(
    mut self,
    tools: Arc<crate::tool::ToolRegistry>,
) -> Result<Self, AgentError> {
    let assembly = crate::prompt::default_coding_assembly(
        tools,
        self.cwd.clone(),
        self.skill_registry.clone(),
    );
    self.base_system_prompt = None;
    self.prompt_assembly = Some(Arc::new(assembly));
    Ok(self)
}
```

In `src/runtime.rs`, update the fallback:

```rust
self.config.prompt_assembly = Some(Arc::new(
    crate::prompt::default_coding_assembly(
        Arc::new(tools.clone()),
        self.config.cwd.clone(),
        self.config.skill_registry.clone(),
    ),
));
```

- [ ] **Step 7: Write integration test**

```rust
#[test]
fn bundled_skills_load_and_render() {
    use telos_agent::skills::SkillRegistry;
    let mut registry = SkillRegistry::new();
    registry.load_bundled_skills();
    assert!(registry.get("explore").is_some());
    let rendered = registry.render_for_prompt();
    assert!(rendered.contains("explore"));
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test --quiet bundled_skills_load_and_render`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add src/skills/registry.rs src/config.rs src/tools/mod.rs src/prompt/mod.rs src/runtime.rs src/lib.rs tests/integration_tests.rs
git commit -m "feat(skills): wire bundled skills into default assembly"
```

---

### Task 12: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test --quiet`
Expected: all tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean

- [ ] **Step 3: Run format check**

Run: `cargo fmt -- --check`
Expected: clean

- [ ] **Step 4: Update `CHANGELOG.md`**

Add a bullet under an "Unreleased" section:

```markdown
- Added tool-specific prompt guidance, system reminders, prompt cache boundary, and bundled Superpowers/explore skills.
```

- [ ] **Step 5: Final commit**

```bash
git add CHANGELOG.md
git commit -m "docs(changelog): summarize prompt system v2 changes"
```

---

## Spec coverage self-check

| Spec requirement | Task |
|------------------|------|
| Tool trait `prompt_text()` | Task 1 |
| `ToolRegistry::iter()` | Task 2 |
| `ToolPromptsSection` | Task 3 |
| Core tool prompts | Task 4 |
| Default assembly wiring | Task 5 |
| `SystemReminder` enum | Task 6 |
| Runtime reminder injection | Task 7 |
| `PromptBlock` / `build_blocks()` | Task 8 |
| `CompletionRequest` cache boundary | Task 9 |
| Bundled skill assets | Task 10 |
| Bundled skill loading | Task 11 |
| Tests & verification | Task 12 |
