# Subagent AgentTool Enhancements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build local AgentTool-style subagent lifecycle capabilities: background execution, task output/stop tools, worktree isolation, richer agent definitions, better delegation guidance, and enhanced plan-tracking tasks.

**Architecture:** learn-claude-code separates two task concerns: **plan/todo tasks** (the work-checklist: `TaskCreate/TaskGet/TaskList/TaskUpdate`, statuses `pending/in_progress/completed`) and **background execution tasks** (`TaskOutput/TaskStop`, statuses `pending/running/completed/failed/killed`). This plan follows that separation — enhance the existing todo `Task` with learn-claude-code V2 fields (`owner`, `active_form`, `metadata`), add a new `BackgroundTask` struct for runtime lifecycle, reuse the existing `TaskManager` for persistence, and add a cancellation registry. Keep synchronous subagent execution compatible, then route `run_in_background` through a Tokio task that updates `BackgroundTask` records.

**Tech Stack:** Rust, Tokio, serde/serde_json/serde_yaml, existing `telos_agent` core runtime, existing mock provider tests.

---

## File Structure

- `core/src/tasks/task.rs`: add `owner`, `active_form`, `metadata` fields to plan `Task`.
- `core/src/tasks/background.rs` (new): `BackgroundTask`, `BackgroundTaskStatus`, persistence helpers.
- `core/src/tasks/mod.rs`: add `TaskManager` methods for plan-task field updates, `add_block`/`remove_block`, and background-task lifecycle.
- `core/src/tasks/tool.rs`: enhance `TaskUpdateTool` (subject/description/owner/active_form/metadata/blocks); add `TaskOutputTool` and `TaskStopTool`.
- `core/src/tasks/persistence.rs`: add background-task disk persistence.
- `core/src/subagent/definition.rs`: parse additional agent frontmatter fields.
- `core/src/subagent/tool.rs`: validate background/worktree now that they are supported and improve prompt guidance.
- `core/src/subagent/tool/agent_mode.rs`: split reusable child execution, add background launch and worktree-aware config.
- `core/src/subagent/worktree.rs`: create/remove/describe git worktrees for isolated subagents.
- `core/src/subagent/mod.rs`: export the worktree helper and keep subagent public exports centralized.
- `core/src/config.rs`: add `task_manager: Option<Arc<TaskManager>>` and cancellation registry to `AgentConfig`.
- `core/tests/subagent_plugin_tests.rs`: integration coverage for background and worktree behavior.
- `core/tests/task_lifecycle_tests.rs`: plan-task field persistence, TaskUpdateTool enhancements, background-task lifecycle, task_output/task_stop coverage.

## Tasks

### Task 1: Plan Task Enhancements (owner, active_form, metadata)

**Files:**
- Modify: `core/src/tasks/task.rs`
- Modify: `core/src/tasks/mod.rs`
- Test: `core/tests/task_lifecycle_tests.rs`

- [x] Write failing tests for `Task` serialization round-trip with `owner`, `active_form`, `metadata` fields.
- [x] Run `cargo test -p telos_agent --test task_lifecycle_tests` and confirm failures.
- [x] Add `owner: Option<String>`, `active_form: Option<String>`, `metadata: Option<HashMap<String, Value>>` to `Task`.
- [x] Re-run tests and confirm pass.
- [x] Commit with `feat: add owner, active_form, metadata to plan tasks`.

### Task 2: TaskUpdateTool Enhancements

**Files:**
- Modify: `core/src/tasks/tool.rs`
- Modify: `core/src/tasks/mod.rs`
- Test: `core/tests/task_lifecycle_tests.rs`

- [ ] Write failing tests: update subject, description, owner, active_form; merge metadata (set key, delete key via null); add_blocks and add_blocked_by (idempotent — skip already-present IDs).
- [ ] Run `cargo test -p telos-agent --test task_lifecycle_tests` and confirm failure.
- [ ] Extend `TaskUpdateTool` input schema: `subject`, `description`, `owner`, `active_form`, `metadata` (record-of-unknown, null deletes key), `add_blocks`, `add_blocked_by`.
- [ ] Add `TaskManager` methods: `update_fields` (merges optional fields), `add_block`, `add_blocked_by` (reverse block).
- [ ] Re-run tests and commit with `feat: enhance TaskUpdateTool with full field and dependency support`.

### Task 3: Background Task Model

**Files:**
- Create: `core/src/tasks/background.rs`
- Modify: `core/src/tasks/mod.rs`
- Modify: `core/src/tasks/persistence.rs`
- Test: `core/tests/task_lifecycle_tests.rs`

- [ ] Write failing tests for `BackgroundTask` serialization and `BackgroundTaskStatus` lifecycle: `pending → running → completed/failed/killed`.
- [ ] Run targeted tests and confirm failure.
- [ ] Define `BackgroundTaskStatus` enum: `Pending`, `Running`, `Completed`, `Failed`, `Killed` (matching learn-claude-code's `pending/running/completed/failed/killed`).
- [ ] Define `BackgroundTask` struct: `id`, `kind`, `agent_id`, `agent_type`, `description`, `status`, `output`, `error`, `worktree_path`, `exit_code`.
- [ ] Add background-task persistence (disk load/save) alongside existing plan-task persistence.
- [ ] Add `TaskManager` methods for background tasks: `create_bg`, `get_bg`, `update_bg_status`, `set_bg_output`, `set_bg_error`.
- [ ] Re-run tests and commit with `feat: add BackgroundTask model separate from plan tasks`.

### Task 4: Task Output And Stop Tools

**Files:**
- Modify: `core/src/tasks/tool.rs`
- Modify: `core/src/tasks/mod.rs`
- Test: `core/tests/task_lifecycle_tests.rs`

- [ ] Write failing tests for `TaskOutputTool`: blocking wait for completion, non-blocking current-status check, timeout, `retrieval_status` (`success`/`timeout`/`not_ready`).
- [ ] Write failing tests for `TaskStopTool`: cancel running task (sets `Killed`), no-op on already-terminal task, backward-compatible `shell_id` alias.
- [ ] Run targeted tests and confirm failure.
- [ ] Implement `TaskOutputTool` with input schema `{ task_id, block (default true), timeout (default 30000, max 600000) }`.
- [ ] Implement blocking poll loop in `TaskOutputTool::invoke` that waits for terminal status or timeout.
- [ ] Implement `TaskStopTool` with input schema `{ task_id, shell_id (deprecated alias) }`.
- [ ] Add cancellation registry to `TaskManager`: `register_cancel`, `cancel`, `unregister` using `tokio::sync::CancellationToken`.
- [ ] Re-run tests and commit with `feat: add task output and stop tools`.

### Task 5: Background Subagent Execution

**Files:**
- Modify: `core/src/subagent/tool.rs`
- Modify: `core/src/subagent/tool/agent_mode.rs`
- Modify: `core/src/config.rs`
- Test: `core/tests/subagent_plugin_tests.rs`

- [ ] Write failing integration test where `run_in_background: true` returns `async_launched` immediately and task output later contains the child final text.
- [ ] Run `cargo test -p telos-agent --test subagent_plugin_tests background` and confirm failure.
- [ ] Add `task_manager: Option<Arc<TaskManager>>` and cancellation registry to `AgentConfig`.
- [ ] Refactor child subagent execution into a reusable async helper.
- [ ] Implement background launch path: create `BackgroundTask(kind="subagent")`, spawn Tokio task, return immediately.
- [ ] Register and unregister cancellation state around the background Tokio task.
- [ ] Store `Completed`, `Failed`, or `Killed` status + output in `BackgroundTask`.
- [ ] Re-run targeted test and commit with `feat: run subagents in background`.

### Task 6: Worktree Isolation

**Files:**
- Create: `core/src/subagent/worktree.rs`
- Modify: `core/src/subagent/mod.rs`
- Modify: `core/src/subagent/tool.rs`
- Modify: `core/src/subagent/tool/agent_mode.rs`
- Test: `core/tests/subagent_plugin_tests.rs`

- [ ] Write failing test using a temporary git repo where `isolation: \"worktree\"` causes the child shell/read context to run in `.worktrees/subagents/<agent-id>`, and `worktree_path` is recorded on the `BackgroundTask`.
- [ ] Run the targeted test and confirm failure.
- [ ] Implement `create_subagent_worktree(parent_cwd, agent_id) -> WorktreeInfo`.
- [ ] Change validation so `isolation: \"worktree\"` is accepted only in agent mode.
- [ ] Set child `config.cwd` to the worktree path and record `worktree_path` on `BackgroundTask`.
- [ ] Re-run targeted test and commit with `feat: support subagent worktree isolation`.

### Task 7: Agent Definition Metadata And Prompt Guidance

**Files:**
- Modify: `core/src/subagent/definition.rs`
- Modify: `core/src/subagent/builtins.rs`
- Modify: `core/src/subagent/tool.rs`
- Test: existing subagent unit tests

- [ ] Write failing tests for `initialPrompt`, `permissionMode`, `skills`, and `effort` frontmatter parsing.
- [ ] Write failing test that prompt guidance mentions self-contained prompts, parallel independent tasks, background output, and worktree isolation.
- [ ] Run `cargo test -p telos-agent subagent::definition subagent::tool`.
- [ ] Add fields to `AgentDefinition` and parser structs.
- [ ] Prepend `initial_prompt` to delegated prompts.
- [ ] Update built-in agent prompts to include stronger worker constraints.
- [ ] Update `SubagentTool::prompt_text`.
- [ ] Re-run targeted tests and commit with `feat: enrich subagent definitions and guidance`.

### Task 8: Full Verification

**Files:**
- No planned source edits unless verification exposes defects.

- [ ] Run `cargo fmt`.
- [ ] Run `cargo test -p telos-agent`.
- [ ] Run `cargo test`.
- [ ] Fix any failures with failing tests first when behavior changes are needed.
- [ ] Commit final fixes if any.
