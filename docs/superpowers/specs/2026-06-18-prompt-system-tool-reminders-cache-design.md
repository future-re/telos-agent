# Prompt System v2: Tool Prompts, System Reminders, and Cache Boundary

- **Date:** 2026-06-18
- **Status:** Design approved, awaiting implementation plan
- **Decision:** Option A — additive, forward-compatible changes

## Context

The telos-agent prompt system already has a modular `PromptAssembly` built from `PromptSection`s, with sections adapted from publicly exposed Claude Code system prompts (`Identity`, `Tone and style`, `Doing tasks`, `Executing actions with care`, `Using your tools`). The next phase adds three capabilities requested by the user:

1. **Tool-specific prompts** — per-tool usage instructions for core tools (`Bash`, `Read`, `Edit`, `Write`, `Glob`, `Grep`, `Subagent`, `Skill`, `WebSearch`, `WebFetch`, `AskUserQuestion`).
2. **System reminder mechanism** — inject `<system-reminder>` user messages at lifecycle points (plan mode, compaction, provider context switches, hook interception, etc.).
3. **Static/dynamic prompt cache boundary** — split the system prompt into cacheable static blocks and non-cacheable dynamic blocks, and expose that boundary to providers.

The user also asked whether Superpowers skill prompts and deep-search/explore prompts can be **internalized** (bundled with the crate rather than loaded from external files).

## Goals

- Give the model precise, per-tool behavioral guidance adapted from `Piebald-AI/claude-code-system-prompts`.
- Make system-level events visible to the model via standard `<system-reminder>` messages.
- Prepare the provider layer for API-level prompt caching without breaking existing providers.
- Bundle Superpowers-like skills and an explore/deep-search skill so they work out of the box.
- Keep all changes additive and backward-compatible; existing tests and custom `Tool` implementations continue to compile.

## Non-goals

- We will not enable actual provider cache control in this iteration unless the provider SDK already supports it trivially. We only expose the boundary so future provider work can use it.
- We will not rewrite the entire `CompletionRequest` API (that was Option B). `system_prompt: Option<String>` remains the primary field; structured blocks are an optional companion field.
- We will not remove the existing `ToolUsageSection` general guidance; tool-specific prompts supplement it.

## Design

### 1. Tool-specific prompts

#### Trait extension

Add an optional method to the `Tool` trait:

```rust
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    /// Optional detailed usage instructions injected into the system prompt.
    /// Return `None` if the tool has no extra behavioral guidance.
    fn prompt_text(&self) -> Option<&'static str> {
        None
    }

    // ... existing methods
}
```

Default `None` means third-party tools are unaffected.

#### Built-in tool prompts

Each core tool in `src/tools/` returns adapted text from the Piebald-AI exposed prompts:

- `Bash` / `ShellTool`
- `Read` / `FileReadTool`
- `Edit` / `FileEditTool`
- `Write` / `FileWriteTool`
- `Glob` / `GlobTool`
- `Grep` / `GrepTool`
- `Subagent` / `SubagentTool`
- `Skill` / `SkillTool`
- `WebSearch` / `WebSearchTool`
- `WebFetch` / `WebFetchTool`
- `AskUserQuestion` / `AskUserQuestionTool`

Prompts focus on:
- when to use the tool vs. alternatives,
- required preconditions (e.g., `Read` before `Edit`),
- exact parameter semantics (e.g., `old_string` must match exactly once),
- output interpretation.

#### Injection into the system prompt

Add a new section:

```rust
pub struct ToolPromptsSection {
    tools: Arc<ToolRegistry>,
}
```

`render` iterates over `tools.definitions()` (or a new registry iterator that yields `(name, Arc<dyn Tool>)`), collects every tool's `prompt_text()`, and renders them under a `## Tool-specific guidance` heading.

Add `ToolPromptsSection::new(tools)` to `default_coding_assembly` immediately after `ToolUsageSection`.

#### Registry support

`ToolRegistry` currently stores `Arc<dyn Tool>` by name. Add a method:

```rust
impl ToolRegistry {
    /// Iterate all registered tools as `(canonical_name, tool)` pairs.
    /// The `Arc` is cloned; the underlying tool is shared.
    pub fn iter(&self) -> impl Iterator<Item = (&String, Arc<dyn Tool>)>;
}
```

Existing `definitions()` stays unchanged for backward compatibility.

### 2. System reminder mechanism

#### Data model

```rust
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
    /// A tool result contains a system-level note (e.g., stale-read warning).
    ToolResult { tool_name: String, note: String },
}

impl SystemReminder {
    pub fn render(&self) -> String;
}
```

#### Runtime injection

`AgentSession` gains a helper:

```rust
fn push_system_reminder(&mut self, reminder: SystemReminder) {
    let text = reminder.render();
    self.messages.push(Message::user(format!("<system-reminder>\n{}\n</system-reminder>", text)));
}
```

Injection points:

| Lifecycle event | Reminder |
|-----------------|----------|
| `run_turn_stream` start (if plan mode flag is set) | `PlanMode` |
| After compaction succeeds | `Compaction { reason }` |
| After a hook emits a message | `HookInterception { phase, name }` |
| When swapping/changing provider | `ProviderContext` |
| When a built-in tool detects a system-level condition | `ToolResult { tool_name, note }` |

The first three are implemented in this iteration. `ProviderContext` and per-tool `ToolResult` reminders are added opportunistically where the runtime already detects those conditions.

Reminders are **user-role messages** so they appear in the conversation transcript and survive compaction.

### 3. Static/dynamic cache boundary

#### Prompt blocks

```rust
#[derive(Debug, Clone)]
pub struct PromptBlock {
    pub name: String,
    pub text: String,
    pub stability: PromptStability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHint {
    /// Safe to cache across turns (identity, tone, tool catalog, tool prompts).
    Static,
    /// Should be re-sent / not cached (date, cwd, git status, memory).
    Dynamic,
}
```

`PromptStability` already exists; `CacheHint` is a semantic alias / provider-facing hint.

#### Assembly API

```rust
impl PromptAssembly {
    /// Render into structured blocks.
    pub async fn build_blocks(&self) -> Vec<PromptBlock>;

    /// Existing convenience: join blocks into one string.
    pub async fn build(&self) -> String;
}
```

`build_blocks` honors the existing static cache and produces one block per section. Empty blocks are omitted.

#### Provider request

```rust
pub struct CompletionRequest {
    pub system_prompt: Option<String>,
    /// Optional structured system prompt blocks for providers that support
    /// per-block cache control (e.g., Anthropic prompt caching).
    pub system_prompt_blocks: Option<Vec<PromptBlock>>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
}
```

`build_request` in `openai_compat.rs` falls back to `system_prompt` if `system_prompt_blocks` is `None`. When blocks are present it joins them with the same separator and ignores cache hints (OpenAI-compatible APIs do not currently expose prompt caching at this granularity).

Future Anthropic provider work can map `Static` blocks to `cache_control: { type: "ephemeral" }`.

### 4. Internalizing Superpowers and deep-search prompts

#### Bundled skills

Add an `assets/skills/` directory containing markdown skill files:

- `explore.md` — deep codebase exploration / research agent instructions
- `brainstorming.md` — design-before-build checklist
- `systematic-debugging.md` — structured debugging workflow
- `writing-plans.md` — implementation planning workflow
- `verification-before-completion.md` — verification checklist

Each file uses the existing skill frontmatter schema (`name`, `description`, `when_to_use`, optional `arguments`).

#### Loading

Add a helper on `AgentConfig` or `SkillRegistry`:

```rust
impl SkillRegistry {
    /// Load the built-in skills shipped with the crate.
    pub fn load_bundled_skills(&mut self) -> Result<(), AgentError>;
}
```

A new `AgentConfig::with_bundled_skills()` helper loads the bundled skills into a fresh `SkillRegistry` with `SkillSource::Bundled` (lowest priority, overridable by project/user skills). Callers can also load them manually into their own `SkillRegistry`.

#### Default assembly integration

If a `SkillRegistry` is provided to `default_coding_assembly`, add `SkillsSection::new(registry)` to the assembly. This makes bundled skills discoverable in the system prompt and invokable via the `Skill` tool.

#### Deep-search skill

`explore.md` replaces the hand-wavy "use Subagent with subagent_type Explore" guidance with concrete instructions:
- ask clarifying questions only when necessary,
- prefer `Glob`/`Grep` for targeted searches,
- use `Subagent` explore mode for broad cross-cutting research,
- summarize findings with citations (`path:line`),
- do not modify code during exploration.

## Implementation notes

- Keep `Tool::prompt_text()` returning `&'static str` so tool prompts live in `.rodata` and do not allocate per call.
- Tool prompts should be stored in the tool module files for locality, not in a giant central string table.
- `ToolPromptsSection` renders under `## Tool-specific guidance` and lists prompts by tool name so the model can associate guidance with the right tool.
- System reminders use the exact `<system-reminder>...</system-reminder>` tag format already referenced in `IdentitySection`.
- The static/dynamic cache boundary is invisible to current providers; no provider tests need to change unless we add a new mock-provider test that asserts on blocks.
- Bundled skills are embedded via `include_str!` at compile time to avoid runtime asset-path issues.

## Testing plan

- Unit tests for `ToolPromptsSection` rendering (with and without tool prompts).
- Unit tests for `SystemReminder::render()` and `push_system_reminder` message shape.
- Unit tests for `PromptAssembly::build_blocks()` ordering and stability.
- Integration test: a session with the default assembly contains tool-specific guidance text in the system prompt sent to `MockProvider`.
- Integration test: compaction emits a `<system-reminder>` user message after the compaction event.
- Integration test: bundled skills are loadable and `SkillRegistry` contains the expected names.
- Run `cargo test --quiet` and `cargo clippy --all-targets -- -D warnings` after each subsystem.

## Compatibility & rollback

- `Tool::prompt_text` has a default implementation → no breaking change for existing tools.
- `CompletionRequest` adds a new field → existing call sites that construct it with struct literals will need updating. We will use `..Default::default()` or a builder where possible.
- `PromptAssembly::build()` signature is unchanged.
- All changes are additive; reverting the new sections from `default_coding_assembly` restores the previous behavior.
