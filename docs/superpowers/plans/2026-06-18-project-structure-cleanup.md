# Project Structure Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure the repository into a self-contained Cargo workspace with clear module boundaries, consistent naming, and no dead code.

**Architecture:** Move from a single-crate root plus an externally-resolved parent workspace to a root `[workspace]` that includes the library and `cli` crates; split the three largest modules into focused submodules; consolidate tool implementations under `src/tools/`; remove the unused CLI display module; align docs and CI with the current repo name.

**Tech Stack:** Rust 1.96, Cargo, Tokio, async-trait, clap, termimad (to be removed), dissimilar (to be removed).

---

## File Structure After Cleanup

```
tiny_agent_core/
├── Cargo.toml                 # workspace + telos_agent package
├── Cargo.lock                 # regenerated workspace lockfile
├── README.md                  # updated branding/paths
├── CHANGELOG.md               # fixed release links
├── rust-toolchain.toml
├── .github/workflows/rust.yml # uses rust-toolchain.toml
├── cli/
│   ├── Cargo.toml             # dissimilar/termimad removed
│   ├── src/
│   │   ├── lib.rs             # display module removed
│   │   ├── config.rs          # DisplaySection removed
│   │   └── ...
│   └── tests/cli_tests.rs     # display tests removed
├── src/
│   ├── runtime/
│   │   ├── mod.rs
│   │   ├── session.rs
│   │   ├── turn.rs
│   │   └── turn.rs
│   ├── executor/
│   │   ├── mod.rs
│   │   ├── types.rs
│   │   ├── batch.rs
│   │   ├── sync.rs
│   │   ├── stream.rs
│   │   ├── invoke.rs
│   │   └── tests.rs
│   ├── plugin/
│   │   ├── registry/
│   │   │   ├── mod.rs
│   │   │   ├── types.rs
│   │   │   ├── lifecycle.rs
│   │   │   ├── discovery.rs
│   │   │   ├── persistence.rs
│   │   │   ├── apply.rs
│   │   │   └── tests.rs
│   │   └── ...
│   ├── tools/
│   │   ├── mod.rs             # includes memory.rs and tasks.rs
│   │   ├── memory.rs          # moved from src/memory/tool.rs
│   │   ├── tasks.rs           # moved from src/tasks/tool.rs
│   │   └── ...
│   ├── memory/
│   │   ├── mod.rs             # compatibility re-export
│   │   └── ...
│   └── tasks/
│       ├── mod.rs             # compatibility re-export
│       └── ...
└── docs/superpowers/          # active docs updated; historical docs annotated
```

---

## Phase 1: Workspace, Docs, and CI

### Task 1: Add workspace to root `Cargo.toml`

**Files:**
- Modify: `Cargo.toml:1`

- [ ] **Step 1: Add `[workspace]` section at the top of `Cargo.toml`**

```toml
[workspace]
members = [".", "cli"]
resolver = "3"

[package]
name = "telos_agent"
version = "0.1.0"
edition = "2024"
```

- [ ] **Step 2: Update the `repository` field**

Change:
```toml
repository = "https://github.com/future-re/telos-agent"
```
to:
```toml
repository = "https://github.com/future-re/tiny_agent_core"
```

- [ ] **Step 3: Verify workspace resolves**

Run: `cargo check --workspace`
Expected: finishes successfully; `Cargo.lock` may be rewritten.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add root workspace and fix repository URL"
```

---

### Task 2: Clean up stale workspace artifacts

**Files:**
- Delete: `Cargo.lock` (will be regenerated)
- Delete/Ignore: `target/` (already ignored)

- [ ] **Step 1: Remove the stale lockfile**

Run: `rm Cargo.lock`

- [ ] **Step 2: Regenerate the workspace lockfile**

Run: `cargo check --workspace`
Expected: a new `Cargo.lock` appears at the repo root containing both `telos_agent` and `telos-cli` dependencies.

- [ ] **Step 3: Commit**

```bash
git add Cargo.lock
git commit -m "chore: regenerate workspace Cargo.lock"
```

---

### Task 3: Update `README.md`

**Files:**
- Modify: `README.md:1,3,10,250,255,262,294,299-301,306`

- [ ] **Step 1: Replace project branding**

Old:
```markdown
# telos-agent

`telos-agent` 是一个用 Rust 编写的意图驱动 agent runtime...
```
New:
```markdown
# tiny_agent_core

`tiny_agent_core` 是一个用 Rust 编写的意图驱动 agent runtime...
```

Also replace line 10:
Old: `核心链路由 \`telos-agent\` 提供`
New: `核心链路由 \`telos_agent\` 库提供`

- [ ] **Step 2: Fix workspace claim and build instructions**

Old (lines 250-257):
```markdown
项目根目录已配置为 Cargo workspace，包含 `telos_agent` 库和 `telos-cli` 可执行 crate。

### 构建整个 workspace

```bash
cd /home/alin/codework/tiny_agent
cargo build
```
```
New:
```markdown
项目根目录已配置为 Cargo workspace，包含 `telos_agent` 库和 `telos-cli` 可执行 crate。

### 构建整个 workspace

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core
cargo build --workspace
```
```

- [ ] **Step 3: Fix CLI install path**

Old (lines 262-264):
```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core/telos-cli
cargo install --path .
```
New:
```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core/cli
cargo install --path .
```

- [ ] **Step 4: Fix README link and test commands**

Old (line 294): `[telos-cli/README.md](telos-cli/README.md)`
New: `[cli/README.md](cli/README.md)`

Old (lines 299-301):
```bash
cd /home/alin/codework/tiny_agent
cargo test --workspace
cargo clippy --workspace --all-targets
```
New:
```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core
cargo test --workspace
cargo clippy --workspace --all-targets
```

- [ ] **Step 5: Replace remaining `telos-agent` references in README**

Replace line 306:
Old: `以下能力在 \`telos-agent\` 当前范围之外`
New: `以下能力在 \`tiny_agent_core\` 当前范围之外`

- [ ] **Step 6: Verify and commit**

Run: `cargo test --workspace`
Expected: PASS

```bash
git add README.md
git commit -m "docs: align README with current repo layout"
```

---

### Task 4: Update `cli/README.md`

**Files:**
- Modify: `cli/README.md`

- [ ] **Step 1: Fix paths and workspace references**

Old:
```markdown
# telos-cli

Terminal interface for [telos-agent](..).
```
New:
```markdown
# telos-cli

Terminal interface for [tiny_agent_core](../..).
```

Old (around line 23-24):
```bash
cd /home/alin/codework/tiny_agent
cargo build -p telos-cli
```
New:
```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core
cargo build -p telos-cli
```

Old (around line 30):
```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core/telos-cli
cargo install --path .
```
New:
```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core/cli
cargo install --path .
```

- [ ] **Step 2: Remove the "Diff display" feature line**

Remove any bullet like:
```markdown
- Diff display: File diffs colored with green/red ANSI escapes.
```

- [ ] **Step 3: Verify and commit**

```bash
git add cli/README.md
git commit -m "docs: align cli README with current paths and remove unused feature claim"
```

---

### Task 5: Update `CHANGELOG.md`

**Files:**
- Modify: `CHANGELOG.md:44-45`

- [ ] **Step 1: Fix release links**

Old:
```markdown
[Unreleased]: https://github.com/tiny-agent/tiny_agent_core/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tiny-agent/tiny_agent_core/releases/tag/v0.1.0
```
New:
```markdown
[Unreleased]: https://github.com/future-re/tiny_agent_core/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/future-re/tiny_agent_core/releases/tag/v0.1.0
```

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: fix CHANGELOG release links"
```

---

### Task 6: Update active `docs/superpowers/` references

**Files:**
- Modify: `docs/superpowers/plans/2026-06-18-telos-cli-phase1.md`
- Modify: `docs/superpowers/plans/2026-06-18-tui-cli-and-workspace.md`
- Modify: `docs/superpowers/plans/2026-06-18-plugin-system-plan.md`
- Modify: `docs/superpowers/plans/2026-06-17-readme-rewrite.md`
- Modify: `docs/superpowers/specs/2026-06-18-tui-cli-and-workspace-design.md`

- [ ] **Step 1: Replace `telos-cli/` with `cli/` in active docs**

Run:
```bash
sed -i 's|telos-cli/|cli/|g' docs/superpowers/plans/2026-06-18-telos-cli-phase1.md
sed -i 's|telos-cli/|cli/|g' docs/superpowers/plans/2026-06-18-tui-cli-and-workspace.md
sed -i 's|telos-cli/|cli/|g' docs/superpowers/plans/2026-06-18-plugin-system-plan.md
```

- [ ] **Step 2: Remove incorrect `tiny_agent_core/` prefix from relative paths in README-rewrite docs**

Run:
```bash
sed -i 's|tiny_agent_core/README.md|README.md|g' docs/superpowers/plans/2026-06-17-readme-rewrite.md docs/superpowers/specs/2026-06-17-readme-rewrite-design.md
sed -i 's|tiny_agent_core/docs/superpowers/|docs/superpowers/|g' docs/superpowers/plans/2026-06-17-readme-rewrite.md docs/superpowers/specs/2026-06-17-readme-rewrite-design.md
```

- [ ] **Step 3: Add historical header to rename/workspace design docs**

Prepend to `docs/superpowers/specs/2026-06-18-rename-telos-cli-to-cli-design.md` and `docs/superpowers/specs/2026-06-18-tui-cli-and-workspace-design.md`:
```markdown
> **Historical note:** This document describes the project state before the
> `telos-cli/` → `cli/` rename and before the root workspace was added.
> Some paths and commands may be outdated.

```

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers
git commit -m "docs: update design docs to use cli/ paths and annotate historical specs"
```

---

### Task 7: Fix CI toolchain

**Files:**
- Modify: `.github/workflows/rust.yml:19,30,50,70`

- [ ] **Step 1: Replace `dtolnay/rust-toolchain@stable` with toolchain-file aware setup**

In all four jobs, change:
```yaml
- uses: dtolnay/rust-toolchain@stable
```
to:
```yaml
- uses: dtolnay/rust-toolchain@stable
- run: rustup show
```

`rustup show` reads `rust-toolchain.toml` and installs/uses the pinned 1.96.0 toolchain.

- [ ] **Step 2: Update clippy and build jobs to use workspace flags**

Change the clippy step from:
```yaml
- run: cargo clippy --all-targets -- -D warnings
```
to:
```yaml
- run: cargo clippy --workspace --all-targets -- -D warnings
```

Change the build/test steps from:
```yaml
- run: cargo build --verbose
- run: cargo test --verbose
```
to:
```yaml
- run: cargo build --workspace --verbose
- run: cargo test --workspace --verbose
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/rust.yml
git commit -m "ci: respect rust-toolchain.toml and run workspace-wide checks"
```

---

## Phase 2: Split Oversized Modules

### Task 8: Split `src/runtime.rs` into `src/runtime/`

**Files:**
- Delete: `src/runtime.rs`
- Create: `src/runtime/mod.rs`
- Create: `src/runtime/session.rs`
- Create: `src/runtime/turn.rs`

- [ ] **Step 1: Create the directory and `mod.rs`**

```bash
mkdir -p src/runtime
```

Create `src/runtime/mod.rs`:
```rust
//! Agent session and turn loop — the orchestration core of the crate.
//!
//! An [`AgentSession`] owns the conversation history and exposes two ways to
//! run a turn:
//! - [`AgentSession::run_turn_stream`] — yields [`TurnEvent`]s incrementally
//!   for live UIs.
//! - [`AgentSession::run_turn`] — collects the stream into a [`TurnResult`]
//!   and persists the session afterwards.

pub use session::AgentSession;
pub use turn::{TurnEvent, TurnResult};

mod session;
mod turn;
mod loop_;
```

Note: use `loop_` as the module file name because `loop` is a keyword; import it as `mod loop_;`.

- [ ] **Step 2: Move `TurnEvent`, `TurnResult`, `estimate_message_tokens`, and tests to `turn.rs`**

Cut lines 37-107 (TurnEvent + TurnResult) and lines 813-1009 (impl TurnEvent + estimate_message_tokens + tests) from `src/runtime.rs` into `src/runtime/turn.rs`.

Add at the top of `src/runtime/turn.rs`:
```rust
use crate::error::AgentError;
use crate::message::{ContentBlock, Message, TextBlock, ThinkingBlock};
use crate::provider::{ModelProvider, StopReason};
use serde::Serialize;
```

- [ ] **Step 3: Move `AgentSession` struct and lifecycle methods to `session.rs`**

Cut lines 109-811 from `src/runtime.rs` into `src/runtime/session.rs`.

Add at the top:
```rust
use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::message::Message;
use crate::metrics::SessionMetrics;
use crate::provider::{CompletionRequest, ModelProvider, ProviderEvent, StopReason, TokenUsage};
use crate::storage::{SessionMetadata, Storage};
use crate::tool::FileReadState;
use crate::tool::ToolRegistry;
use async_stream::try_stream;
use futures_core::stream::Stream;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, error, info, info_span, warn};
```

- [ ] **Step 4: Move the turn loop body to `loop_.rs`**

The turn loop body (`run_turn_stream` and `run_turn`) is tightly coupled to `AgentSession` and uses `try_stream!`. Keep these methods inside `session.rs` rather than a separate file to avoid lifetime issues. **Do NOT create `loop_.rs` for these methods.**

Revise Step 1: remove the `mod loop_;` line from `mod.rs`.

- [ ] **Step 5: Update imports and delete `src/runtime.rs`**

After moving content, delete `src/runtime.rs`.
Ensure `src/lib.rs` still declares `pub mod runtime;`.

- [ ] **Step 6: Verify**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/runtime src/runtime.rs
git commit -m "refactor: split runtime.rs into session and turn modules"
```

---

### Task 9: Split `src/executor.rs` into `src/executor/`

**Files:**
- Delete: `src/executor.rs`
- Create: `src/executor/mod.rs`
- Create: `src/executor/types.rs`
- Create: `src/executor/batch.rs`
- Create: `src/executor/sync.rs`
- Create: `src/executor/stream.rs`
- Create: `src/executor/invoke.rs`
- Create: `src/executor/tests.rs`

- [ ] **Step 1: Create directory and `mod.rs`**

```bash
mkdir -p src/executor
```

Create `src/executor/mod.rs`:
```rust
//! Tool execution engine with batching and streaming support.

pub use types::{ToolExecutionEvent, ToolExecutionOutput, ToolExecutionStreamItem};
pub use sync::execute_tool_calls;
pub use stream::execute_tool_calls_stream;

mod types;
mod batch;
mod sync;
mod stream;
mod invoke;
#[cfg(test)]
mod tests;
```

- [ ] **Step 2: Move types to `types.rs`**

Cut lines 22-67 (ToolExecutionEvent, ToolExecutionOutput, PreparedCall, Batch, ToolExecutionStreamItem) into `src/executor/types.rs`.

Add imports:
```rust
use crate::error::AgentError;
use crate::message::{ToolCall, ToolResult};
use crate::tool::ToolContext;
use serde_json::Value;
```

- [ ] **Step 3: Move batch planning and `run_concurrent_batch` to `batch.rs`**

Cut lines 285-314 (`run_concurrent_batch`) into `batch.rs`. Also extract the duplicated batch-planning logic from `execute_tool_calls` and `execute_tool_calls_stream` into a `pub(crate) fn prepare_batches` in `batch.rs`.

`batch.rs`:
```rust
use crate::config::AgentConfig;
use crate::message::ToolCall;
use crate::tool::{ToolContext, ToolRegistry};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct PreparedCall {
    pub index: usize,
    pub call: ToolCall,
    pub context: ToolContext,
}

#[derive(Debug, Clone)]
pub(crate) struct Batch {
    pub concurrency_safe: bool,
    pub calls: Vec<PreparedCall>,
}

pub(crate) fn prepare_batches(
    calls: Vec<ToolCall>,
    tools: &ToolRegistry,
    config: &AgentConfig,
    session_id: &str,
    turn_id: u64,
    messages: Arc<Vec<crate::message::Message>>,
    read_file_state: crate::tool::FileReadState,
) -> Vec<Batch> {
    let mut batches = Vec::new();
    for (index, call) in calls.into_iter().enumerate() {
        let context = ToolContext {
            session_id: session_id.to_string(),
            turn_id,
            cwd: config.cwd.clone(),
            env: config.env.clone(),
            messages: Arc::clone(&messages),
            progress: None,
            read_file_state: read_file_state.clone(),
            timeout: config
                .tool_timeout_ms
                .filter(|&ms| ms > 0)
                .map(std::time::Duration::from_millis),
            max_file_read_bytes: config.max_file_read_bytes,
        };
        let concurrency_safe = tools
            .get(&call.name)
            .ok()
            .map(|tool| tool.is_concurrency_safe(&call.arguments))
            .unwrap_or(false);
        if let Some(batch) = batches.last_mut()
            && batch.concurrency_safe
            && concurrency_safe
        {
            batch.calls.push(PreparedCall { index, call, context });
        } else {
            batches.push(Batch {
                concurrency_safe,
                calls: vec![PreparedCall { index, call, context }],
            });
        }
    }
    batches
}

pub(crate) async fn run_concurrent_batch(
    batch: Batch,
    tools: &ToolRegistry,
    config: &AgentConfig,
    output: &mut super::types::ToolExecutionOutput,
) {
    use futures_util::stream::{FuturesUnordered, StreamExt};
    let mut pending = FuturesUnordered::new();
    let mut queued = batch.calls.into_iter();

    for _ in 0..config.tool_concurrency_limit {
        if let Some(prepared) = queued.next() {
            pending.push(super::sync::run_one_tool(prepared, tools, config));
        }
    }

    let mut completed = Vec::new();
    while let Some((index, events, result)) = pending.next().await {
        output.events.extend(events);
        completed.push((index, result));
        if let Some(prepared) = queued.next() {
            pending.push(super::sync::run_one_tool(prepared, tools, config));
        }
    }
    completed.sort_by_key(|(index, _)| *index);
    output.results.extend(completed.into_iter().map(|(_, result)| result));
}
```

- [ ] **Step 4: Move sync path to `sync.rs`**

Cut `execute_tool_calls`, `run_one_tool`, `run_one_tool_inner` into `sync.rs`.

Top imports:
```rust
use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::message::{ToolCall, ToolResult};
use crate::tool::{ToolContext, ToolRegistry};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
```

- [ ] **Step 5: Move streaming path to `stream.rs`**

Cut `execute_tool_calls_stream`, `spawn_live_tool`, `run_live_tool_inner` into `stream.rs`.

Top imports:
```rust
use crate::config::AgentConfig;
use crate::message::{ToolCall, ToolResult};
use crate::tool::{ToolContext, ToolRegistry};
use async_stream::stream;
use futures_core::stream::Stream;
use futures_util::FutureExt;
use futures_util::stream::StreamExt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
```

- [ ] **Step 6: Move `invoke_existing_tool` and `json_error_payload` to `invoke.rs`**

Cut lines 548-779 into `invoke.rs`.

Top imports:
```rust
use crate::approval;
use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::message::{ToolCall, ToolResult};
use crate::permissions::RuleDecision;
use crate::tool::{PermissionDecision, ToolContext, ToolRegistry};
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::Instrument;
```

- [ ] **Step 7: Move tests to `tests.rs`**

Cut the `#[cfg(test)] mod tests` block from `src/executor.rs` into `src/executor/tests.rs`.

Top imports:
```rust
use crate::executor::{execute_tool_calls, ToolExecutionOutput};
use crate::message::ToolCall;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
```

- [ ] **Step 8: Verify and commit**

Run: `cargo test --workspace`
Expected: PASS

```bash
git add src/executor src/executor.rs
git commit -m "refactor: split executor.rs into focused submodules"
```

---

### Task 10: Split `src/plugin/registry.rs` into `src/plugin/registry/`

**Files:**
- Delete: `src/plugin/registry.rs`
- Create: `src/plugin/registry/mod.rs`
- Create: `src/plugin/registry/types.rs`
- Create: `src/plugin/registry/lifecycle.rs`
- Create: `src/plugin/registry/discovery.rs`
- Create: `src/plugin/registry/persistence.rs`
- Create: `src/plugin/registry/apply.rs`
- Create: `src/plugin/registry/tests.rs`

- [ ] **Step 1: Create directory and `mod.rs`**

```bash
mkdir -p src/plugin/registry
```

Create `src/plugin/registry/mod.rs`:
```rust
//! PluginRegistry — manages loaded plugins and their enable/disable lifecycle.

pub use types::{LoadedPlugin, PluginEntry, PluginStatus};
pub use lifecycle::PluginRegistry;

mod types;
mod lifecycle;
mod discovery;
mod persistence;
mod apply;
#[cfg(test)]
mod tests;
```

- [ ] **Step 2: Move types to `types.rs`**

Cut lines 9-54 (PluginStatus, LoadedPlugin, PluginEntry, PluginEntry::new) into `types.rs`.

Top imports:
```rust
use crate::plugin::manifest::PluginManifest;
use crate::plugin::{PluginError, PluginId, PluginSource};
use std::path::PathBuf;
```

- [ ] **Step 3: Move lifecycle and queries to `lifecycle.rs`**

Cut lines 56-196 (PluginRegistry struct, new, installed_dir, state_path, register, enable, disable, mark_degraded, mark_error, remove, get/get_mut, list_*, is_installed, len, is_empty) into `lifecycle.rs`.

Top imports:
```rust
use crate::plugin::{PluginError, PluginId};
use crate::plugin::registry::types::{PluginEntry, PluginStatus};
use std::collections::HashMap;
use std::path::PathBuf;
```

- [ ] **Step 4: Move discovery to `discovery.rs`**

Cut lines 197-313 (`discover_installed`, `load_plugin_from_dir`) into `discovery.rs`.

Top imports:
```rust
use crate::plugin::manifest::PluginManifest;
use crate::plugin::{PluginError, PluginId, PluginSource};
use crate::plugin::registry::types::LoadedPlugin;
use crate::plugin::registry::lifecycle::PluginRegistry;
use std::path::{Path, PathBuf};
```

- [ ] **Step 5: Move persistence to `persistence.rs`**

Cut lines 314-385 (`save_state`, `load_state`) into `persistence.rs`.

Top imports:
```rust
use crate::plugin::{PluginError, PluginId};
use crate::plugin::registry::lifecycle::PluginRegistry;
use crate::plugin::registry::types::{PluginEntry, PluginStatus};
use serde_json;
use std::collections::HashMap;
```

- [ ] **Step 6: Move `apply` to `apply.rs`**

Cut lines 386-545 (`apply`) into `apply.rs`.

Top imports:
```rust
use crate::plugin::{PluginError, PluginPromptSection};
use crate::plugin::registry::lifecycle::PluginRegistry;
use crate::plugin::registry::types::PluginStatus;
```

- [ ] **Step 7: Move tests to `tests.rs`**

Cut the `#[cfg(test)] mod tests` block into `tests.rs`.

Top imports:
```rust
use crate::plugin::registry::types::{LoadedPlugin, PluginStatus};
use crate::plugin::registry::lifecycle::PluginRegistry;
use crate::plugin::{PluginError, PluginId, PluginManifest, PluginSource};
use tempfile::TempDir;
```

- [ ] **Step 8: Update `src/plugin/mod.rs` to declare the new module**

Change:
```rust
pub mod registry;
```
to:
```rust
pub mod registry;
pub use registry::{PluginRegistry, LoadedPlugin, PluginEntry, PluginStatus};
```

- [ ] **Step 9: Verify and commit**

Run: `cargo test --workspace`
Expected: PASS

```bash
git add src/plugin/registry src/plugin/registry.rs src/plugin/mod.rs
git commit -m "refactor: split plugin registry into focused submodules"
```

---

## Phase 3: Tool Consolidation and CLI Cleanup

### Task 11: Move memory tool into `src/tools/`

**Files:**
- Create: `src/tools/memory.rs`
- Modify: `src/tools/mod.rs`
- Modify: `src/memory/mod.rs`

- [ ] **Step 1: Move the file**

```bash
git mv src/memory/tool.rs src/tools/memory.rs
```

- [ ] **Step 2: Update imports in `src/tools/memory.rs`**

Replace any `crate::memory::` imports with the public types they reference. Ensure the file imports:
```rust
use crate::memory::format::{MemoryCategory, MemoryEntry, MemoryStatus};
use crate::memory::index::MemoryStore;
```

- [ ] **Step 3: Update `src/tools/mod.rs`**

Add:
```rust
mod memory;
```

Add re-export:
```rust
pub use memory::{MemoryEditTool, MemoryGrepTool, MemoryReadTool, MemoryStatusTool, MemoryWriteTool};
```

Add registration helper:
```rust
pub fn register_memory_tools(
    registry: &mut ToolRegistry,
    store: std::sync::Arc<tokio::sync::Mutex<MemoryStore>>,
) {
    registry.register(MemoryReadTool::new(store.clone()));
    registry.register(MemoryWriteTool::new(store.clone()));
    registry.register(MemoryGrepTool::new(store.clone()));
    registry.register(MemoryEditTool::new(store.clone()));
    registry.register(MemoryStatusTool::new(store));
}
```

(Adjust constructor names to match the actual tool structs.)

- [ ] **Step 4: Update `src/memory/mod.rs`**

Change:
```rust
pub mod tool;
pub use tool::{MemoryEditTool, MemoryGrepTool, MemoryReadTool, MemoryStatusTool, MemoryWriteTool};
```
to:
```rust
pub use crate::tools::memory::{MemoryEditTool, MemoryGrepTool, MemoryReadTool, MemoryStatusTool, MemoryWriteTool};
```

- [ ] **Step 5: Verify and commit**

Run: `cargo check --workspace`
Expected: PASS

```bash
git add src/tools/memory.rs src/tools/mod.rs src/memory/mod.rs src/memory/tool.rs
git commit -m "refactor: move memory tool implementations into src/tools/"
```

---

### Task 12: Move task tool into `src/tools/`

**Files:**
- Create: `src/tools/tasks.rs`
- Modify: `src/tools/mod.rs`
- Modify: `src/tasks/mod.rs`

- [ ] **Step 1: Move the file**

```bash
git mv src/tasks/tool.rs src/tools/tasks.rs
```

- [ ] **Step 2: Update imports in `src/tools/tasks.rs`**

Ensure imports reference public `crate::tasks` types:
```rust
use crate::tasks::{Task, TaskManager, TaskStatus};
```

- [ ] **Step 3: Update `src/tools/mod.rs`**

Add:
```rust
mod tasks;
```

Change re-export from:
```rust
pub use crate::tasks::tool::{TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool};
```
to:
```rust
pub use tasks::{TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool};
```

- [ ] **Step 4: Update `src/tasks/mod.rs`**

Change:
```rust
pub mod tool;
pub use tool::{TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool};
```
to:
```rust
pub use crate::tools::tasks::{TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool};
```

- [ ] **Step 5: Verify and commit**

Run: `cargo test --workspace`
Expected: PASS

```bash
git add src/tools/tasks.rs src/tools/mod.rs src/tasks/mod.rs src/tasks/tool.rs
git commit -m "refactor: move task tool implementations into src/tools/"
```

---

### Task 13: Remove unused CLI display module

**Files:**
- Delete: `cli/src/display.rs`
- Modify: `cli/src/lib.rs`
- Modify: `cli/Cargo.toml`
- Modify: `cli/src/config.rs`
- Modify: `cli/tests/cli_tests.rs`

- [ ] **Step 1: Delete the module**

```bash
rm cli/src/display.rs
```

- [ ] **Step 2: Remove `pub mod display;` from `cli/src/lib.rs`**

Edit `cli/src/lib.rs` to remove line 8 (`pub mod display;`).

- [ ] **Step 3: Remove dependencies from `cli/Cargo.toml`**

Delete:
```toml
dissimilar = "1"
termimad = "0.30"
```

- [ ] **Step 4: Remove `DisplaySection` from `cli/src/config.rs`**

Delete:
```rust
pub display: Option<DisplaySection>,
```
from `FileConfig`.

Delete the entire `DisplaySection` struct:
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct DisplaySection {
    pub theme: Option<String>,
    pub render_markdown: Option<bool>,
}
```

Delete the `merge_display` function and its call in `merge_configs`.

- [ ] **Step 5: Remove display tests from `cli/tests/cli_tests.rs`**

Delete the `termimad_renders_markdown`, `display_render_disabled_returns_plain`, `render_diff_colors_additions_green`, and `render_diff_colors_removals_red` tests (lines 192-218).

Also remove the `termimad` and `dissimilar` compile-check lines from `new_dependencies_compile` (lines 11 and 20).

- [ ] **Step 6: Verify and commit**

Run: `cargo test --workspace`
Expected: PASS

```bash
git add cli/src/display.rs cli/src/lib.rs cli/Cargo.toml cli/src/config.rs cli/tests/cli_tests.rs
git commit -m "refactor: remove unused CLI display module and dependencies"
```

---

## Final Validation

### Task 14: Full workspace check

- [ ] **Step 1: Format**

Run: `cargo fmt --all`

- [ ] **Step 2: Clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings/errors.

- [ ] **Step 3: Tests**

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 4: Clean git status**

Run: `git status`
Expected: nothing to commit, working tree clean (ignored artifacts may still be present).

- [ ] **Step 5: Final commit if any formatting changes**

```bash
git diff --quiet || git commit -am "style: apply cargo fmt after refactor"
```

---

## Spec Coverage Check

| Spec requirement | Plan task |
|---|---|
| Add root `[workspace]` | Task 1 |
| Regenerate/delete stale `Cargo.lock` | Task 2 |
| Update repository URL in `Cargo.toml` | Task 1 |
| Update `README.md` | Task 3 |
| Update `cli/README.md` | Task 4 |
| Update `CHANGELOG.md` | Task 5 |
| Update active design docs | Task 6 |
| Fix CI toolchain | Task 7 |
| Split `src/runtime.rs` | Task 8 |
| Split `src/executor.rs` | Task 9 |
| Split `src/plugin/registry.rs` | Task 10 |
| Move memory tool to `src/tools/` | Task 11 |
| Move task tool to `src/tools/` | Task 12 |
| Remove unused CLI display module | Task 13 |
| Final validation | Task 14 |
