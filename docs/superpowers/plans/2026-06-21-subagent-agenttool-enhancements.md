# Subagent AgentTool Enhancements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build local AgentTool-style subagent lifecycle capabilities: background execution, task output/stop tools, worktree isolation, richer agent definitions, and better delegation guidance.

**Architecture:** Reuse the existing `TaskManager` as the persisted lifecycle store and add a runtime handle for cancellation. Keep synchronous subagent execution compatible, then route `run_in_background` through a Tokio task that updates task records. Implement worktree isolation inside subagent execution and keep richer Claude Code-style metadata parsed but only consume fields that are safe locally.

**Tech Stack:** Rust, Tokio, serde/serde_json/serde_yaml, existing `telos_agent` core runtime, existing mock provider tests.

---

## File Structure

- `core/src/tasks/task.rs`: extend task status and metadata fields.
- `core/src/tasks/mod.rs`: add output updates and cancellation registry support.
- `core/src/tools/tasks.rs`: add or extend task lifecycle tools.
- `core/src/subagent/definition.rs`: parse additional agent frontmatter fields.
- `core/src/subagent/tool.rs`: validate background/worktree now that they are supported and improve prompt guidance.
- `core/src/subagent/tool/agent_mode.rs`: split reusable child execution, add background launch and worktree-aware config.
- `core/src/subagent/worktree.rs`: create/remove/describe git worktrees for isolated subagents.
- `core/src/subagent/mod.rs`: export the worktree helper and keep subagent public exports centralized.
- `core/tests/subagent_plugin_tests.rs`: integration coverage for background and worktree behavior.
- `core/tests/task_lifecycle_tests.rs`: task output/stop persistence and tool coverage.

## Tasks

### Task 1: Task Lifecycle Data

**Files:**
- Modify: `core/src/tasks/task.rs`
- Modify: `core/src/tasks/mod.rs`
- Test: `core/tests/task_lifecycle_tests.rs`

- [ ] Write failing tests for `Failed` and `Cancelled` task persistence plus subagent metadata round trip.
- [ ] Run `cargo test -p telos-agent --test task_lifecycle_tests` and confirm the new tests fail because statuses/fields are missing.
- [ ] Add `Failed` and `Cancelled` to `TaskStatus`.
- [ ] Add optional task metadata fields: `kind`, `agent_id`, `agent_type`, `worktree_path`, `error`.
- [ ] Add `TaskManager::set_output`, `TaskManager::fail`, `TaskManager::cancel`, and `TaskManager::update_task`.
- [ ] Re-run `cargo test -p telos-agent --test task_lifecycle_tests` and confirm pass.
- [ ] Commit with `feat: extend task lifecycle state`.

### Task 2: Task Output And Stop Tools

**Files:**
- Modify: `core/src/tools/tasks.rs`
- Modify: `core/src/tasks/mod.rs`
- Test: `core/tests/task_lifecycle_tests.rs`

- [ ] Write failing tests for `task_output` returning status/output/metadata and `task_stop` cancelling a registered running task.
- [ ] Run `cargo test -p telos-agent --test task_lifecycle_tests` and confirm failure.
- [ ] Implement `TaskOutputTool`.
- [ ] Implement `TaskStopTool`.
- [ ] Add cancellation registry methods to `TaskManager`: register, cancel, unregister.
- [ ] Ensure tool schemas use `task_id`.
- [ ] Re-run targeted tests and commit with `feat: add task output and stop tools`.

### Task 3: Background Subagent Execution

**Files:**
- Modify: `core/src/subagent/tool.rs`
- Modify: `core/src/subagent/tool/agent_mode.rs`
- Modify: `core/src/config.rs`
- Test: `core/tests/subagent_plugin_tests.rs`

- [ ] Write failing integration test where `run_in_background: true` returns `async_launched` immediately and task output later contains the child final text.
- [ ] Run `cargo test -p telos-agent --test subagent_plugin_tests background` and confirm failure.
- [ ] Add `task_manager: Option<Arc<TaskManager>>` to `AgentConfig`.
- [ ] Refactor child subagent execution into a reusable async helper.
- [ ] Implement background launch path using cloned provider/tools/config and task manager.
- [ ] Register and unregister cancellation state around the background task.
- [ ] Store completed, failed, or cancelled status in the task.
- [ ] Re-run targeted test and commit with `feat: run subagents in background`.

### Task 4: Worktree Isolation

**Files:**
- Create: `core/src/subagent/worktree.rs`
- Modify: `core/src/subagent/mod.rs`
- Modify: `core/src/subagent/tool.rs`
- Modify: `core/src/subagent/tool/agent_mode.rs`
- Test: `core/tests/subagent_plugin_tests.rs`

- [ ] Write failing test using a temporary git repo where `isolation: "worktree"` causes the child shell/read context to run in `.worktrees/subagents/<agent-id>`.
- [ ] Run the targeted test and confirm failure.
- [ ] Implement `create_subagent_worktree(parent_cwd, agent_id) -> WorktreeInfo`.
- [ ] Change validation so `isolation: "worktree"` is accepted only in agent mode.
- [ ] Set child `config.cwd` to the worktree path and record `worktree_path` in task/result metadata.
- [ ] Re-run targeted test and commit with `feat: support subagent worktree isolation`.

### Task 5: Agent Definition Metadata And Prompt Guidance

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

### Task 6: Full Verification

**Files:**
- No planned source edits unless verification exposes defects.

- [ ] Run `cargo fmt`.
- [ ] Run `cargo test -p telos-agent`.
- [ ] Run `cargo test`.
- [ ] Fix any failures with failing tests first when behavior changes are needed.
- [ ] Commit final fixes if any.
