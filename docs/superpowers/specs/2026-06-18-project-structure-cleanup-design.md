# Design: Comprehensive Project Structure Cleanup

## Goal

Make the repository layout self-consistent, easier to navigate, and aligned with the current crate/directory names, while preserving the public API and keeping the project buildable/testable after every phase.

## Current Pain Points

1. **Workspace configuration is split and partly outside the repo**
   - Root `Cargo.toml` has no `[workspace]`.
   - Cargo currently resolves a workspace one directory above the repo (`/home/alin/codework/tiny_agent/Cargo.toml`), which is not tracked.
   - `Cargo.lock` and `target/` inside `tiny_agent_core` are stale/unused.

2. **Oversized top-level modules**
   - `src/runtime.rs` (~1009 lines)
   - `src/executor.rs` (~930 lines)
   - `src/plugin/registry.rs` (~775 lines)

3. **Tool implementation locations are inconsistent**
   - Most tools live in `src/tools/`.
   - `src/memory/tool.rs` and `src/tasks/tool.rs` live inside their domain modules.

4. **Dead code in the CLI**
   - `cli/src/display.rs` is declared and tested but never used by `cli/src/runner.rs`.
   - `termimad` and `dissimilar` dependencies exist only for this unused module.

5. **Naming and documentation drift**
   - `Cargo.toml` `repository` points to `future-re/telos-agent` instead of `future-re/tiny_agent_core`.
   - `README.md` and `cli/README.md` still reference the old `telos-cli/` path and the parent workspace directory.
   - `CHANGELOG.md` release links use the wrong org (`tiny-agent`).
   - Active design docs in `docs/superpowers/` still use `telos-cli/` and `tiny_agent_core/` prefixes.

6. **CI/toolchain mismatch**
   - `.github/workflows/rust.yml` uses `stable`, while `rust-toolchain.toml` pins `1.96.0` and `Cargo.toml` declares `rust-version = "1.96"`.

## Proposed Changes

### 1. Make the repo a self-contained Cargo workspace

Add to root `Cargo.toml`:

```toml
[workspace]
members = [".", "cli"]
resolver = "3"
```

- This makes `cargo test --workspace` and `cargo clippy --workspace --all-targets` work from the repo root without relying on an external parent workspace.
- Delete the stale `Cargo.lock` at the repo root after the first workspace build regenerates it.
- The existing `target/` directory is already ignored; after the workspace migration it will be regenerated locally (or continue using the parent workspace target until a clean build).

### 2. Split oversized modules into directories

#### `src/runtime.rs` → `src/runtime/`

```
src/runtime/
  mod.rs        # public re-exports
  session.rs    # AgentSession lifecycle
  turn.rs       # TurnEvent, TurnResult, estimate_message_tokens
  loop.rs       # run_turn_stream, run_turn, and phase helpers
```

Public API preserved via `mod.rs`:

```rust
pub use session::AgentSession;
pub use turn::{TurnEvent, TurnResult};
```

#### `src/executor.rs` → `src/executor/`

The directory already exists as an empty placeholder.

```
src/executor/
  mod.rs        # public re-exports
  types.rs      # ToolExecutionEvent, ToolExecutionOutput, ToolExecutionStreamItem, PreparedCall, Batch
  batch.rs      # Batch planning + run_concurrent_batch
  sync.rs       # execute_tool_calls, run_one_tool, run_one_tool_inner
  stream.rs     # execute_tool_calls_stream, spawn_live_tool, run_live_tool_inner
  invoke.rs     # invoke_existing_tool, json_error_payload
  tests.rs      # unit tests
```

Public API preserved via `mod.rs`:

```rust
pub use types::{ToolExecutionEvent, ToolExecutionOutput, ToolExecutionStreamItem};
pub use sync::execute_tool_calls;
pub use stream::execute_tool_calls_stream;
```

#### `src/plugin/registry.rs` → `src/plugin/registry/`

```
src/plugin/registry/
  mod.rs           # public re-exports
  types.rs         # PluginStatus, LoadedPlugin, PluginEntry
  lifecycle.rs     # register, enable, disable, mark_*, remove, queries
  discovery.rs     # discover_installed, load_plugin_from_dir
  persistence.rs   # save_state, load_state
  apply.rs         # apply and component-loading helpers
  tests.rs         # tests
```

Public API preserved via `mod.rs`:

```rust
pub use types::{LoadedPlugin, PluginEntry, PluginStatus};
pub use lifecycle::PluginRegistry;
```

### 3. Unify tool implementations under `src/tools/`

Move:
- `src/memory/tool.rs` → `src/tools/memory.rs`
- `src/tasks/tool.rs` → `src/tools/tasks.rs`

Update re-exports:
- `src/tools/mod.rs` declares `mod memory; mod tasks;` and re-exports all tool structs.
- `src/memory/mod.rs` and `src/tasks/mod.rs` keep compatibility re-exports so existing paths (`telos_agent::memory::MemoryReadTool`, `telos_agent::tasks::TaskCreateTool`) continue to work.
- Add `register_memory_tools(registry, Arc<Mutex<MemoryStore>>)` in `src/tools/mod.rs` for symmetry with `register_task_tools`.

### 4. Remove unused CLI display module

- Delete `cli/src/display.rs`.
- Remove `pub mod display;` from `cli/src/lib.rs`.
- Remove `termimad` and `dissimilar` from `cli/Cargo.toml`.
- Remove or simplify the `[display]` config section in `cli/src/config.rs`.
- Remove display-related tests in `cli/tests/cli_tests.rs`.
- Update `cli/README.md` to remove the advertised "Diff display" feature until it is actually implemented.

Rationale: the module is a partially implemented stub. Removing it reduces dependencies and dead code. If markdown/diff rendering becomes a priority later, it can be re-added with a proper streaming-safe design.

### 5. Align names and documentation

- Update root `Cargo.toml`:
  - `repository = "https://github.com/future-re/tiny_agent_core"`
- Update `README.md`:
  - Use `tiny_agent_core` as the project name (the library crate remains `telos_agent`).
  - Correct the workspace claim.
  - Replace `telos-cli/` with `cli/`.
  - Fix absolute paths to point inside the repo.
  - Make build/test commands unambiguous (`cargo test --workspace` from repo root).
- Update `cli/README.md`:
  - Fix install/build paths.
  - Clarify workspace commands.
- Update `CHANGELOG.md`:
  - Fix compare/release URLs to `https://github.com/future-re/tiny_agent_core/...`.
- Update active `docs/superpowers/` plans/specs:
  - Replace `telos-cli/` with `cli/`.
  - Remove the `tiny_agent_core/` prefix from in-repo relative paths.
  - Historical docs (`rename-telos-cli-to-cli-design.md`, `tui-cli-and-workspace-design.md`) get a header noting they describe pre-rename/pre-workspace state.

### 6. Fix CI toolchain

Update `.github/workflows/rust.yml` so the build uses the toolchain declared in `rust-toolchain.toml` (1.96.0) instead of `stable`. Options:
- Replace `rustup install stable` with `rustup show` (rustup reads `rust-toolchain.toml`).
- Or use `dtolnay/rust-toolchain@stable` and let the file override the toolchain.

Preferred: rely on `rust-toolchain.toml` by running `rustup show` before building.

## Phased Implementation

Because the changes are broad, implement in three phases. Each phase ends with `cargo check`, `cargo test`, `cargo clippy`, and a git commit.

### Phase 1: Workspace, docs, CI

- Add `[workspace]` to root `Cargo.toml`.
- Update repository URL in `Cargo.toml`.
- Update `README.md`, `cli/README.md`, `CHANGELOG.md`.
- Update active `docs/superpowers/` references.
- Fix CI toolchain.
- Regenerate/delete stale `Cargo.lock`.

### Phase 2: Module splits

- Split `src/runtime.rs` into `src/runtime/`.
- Split `src/executor.rs` into `src/executor/`.
- Split `src/plugin/registry.rs` into `src/plugin/registry/`.

### Phase 3: Tool relocation and CLI cleanup

- Move `src/memory/tool.rs` and `src/tasks/tool.rs` into `src/tools/`.
- Add `register_memory_tools` helper.
- Remove `cli/src/display.rs` and its dependencies/tests/config.

## Validation

After each phase:

1. `cargo check --workspace`
2. `cargo test --workspace`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `git status` is clean except for ignored artifacts.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Splitting `runtime.rs` breaks the `try_stream!` self-borrow | Keep the stream function and its phase helpers together in `runtime/loop.rs`; do not scatter them. |
| `executor` sync/stream share internal types | Define `PreparedCall`/`Batch` as `pub(crate)` in `executor/types.rs`. |
| Tool relocation breaks public imports | Keep compatibility re-exports in `memory/mod.rs` and `tasks/mod.rs`. |
| Removing CLI display breaks tests | Delete or update the display tests at the same time. |
| Workspace migration invalidates stale `Cargo.lock` | Let Cargo regenerate it and commit the new lockfile. |
| Doc path updates miss references | Use `grep -R` for `telos-cli/`, `tiny-agent-core`, and the old absolute paths. |

## Out of Scope

- Rebranding the crate names themselves (e.g., renaming `telos_agent` → `tiny_agent_core`). The library and CLI package names stay as-is; only documentation and metadata references are aligned.
- Large-scale logic refactors inside the split files. The goal is file organization, not rewriting algorithms.
- Cleaning the 13 GB `.claude/` and `target/` directories. They are already ignored; this is a disk-cleanup task, not a project-structure task.
