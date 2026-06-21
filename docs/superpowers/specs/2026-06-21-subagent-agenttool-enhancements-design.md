# Subagent AgentTool Enhancements Design

## Goal

Strengthen Tiny Agent's subagent system by borrowing the practical local AgentTool capabilities from `learn-claude-code`: typed agent definitions, background execution, task output retrieval, cancellation, worktree isolation, and clearer delegation guidance.

## Scope

This phase is local-only. It does not implement remote workers, tmux teammates, coordinator mode, cross-agent mailboxes, or full swarm UI. Those require a larger host/runtime layer and should follow once the local subagent lifecycle is solid.

## Current State

Tiny Agent already has:

- `SubagentTool` with `agent` and `fork` modes.
- `SubagentRegistry` with built-in and plugin-loaded agent markdown definitions.
- Tool filtering through `allowed_tools` and `disallowed_tools`.
- Progress forwarding from child agent turns to the parent tool call.
- A persistent `TaskManager` with basic task state.

The gaps are:

- `run_in_background` and `isolation: "worktree"` appear in schemas but are rejected.
- Subagent results are synchronous only; long work blocks the parent turn.
- There is no tool for reading a background subagent's final or partial output.
- There is no stop/cancel path for background subagents.
- Agent definition metadata is much thinner than Claude Code's local agent definitions.
- Delegation guidance does not teach the model how to split work into self-contained tasks.

## Architecture

Add a shared subagent runtime handle to `AgentConfig`, backed by the existing `TaskManager`. `SubagentTool` will use it when `run_in_background` is requested. Background subagents run in a Tokio task, update their `BackgroundTask` record as they progress, and write their final text or failure into the background task output.

Expose two lifecycle tools operating on `BackgroundTask`:

- `task_output`: fetch one background task's status and output.
- `task_stop`: request cancellation for a running background task.

These tools use the `BackgroundTask` model — they are independent of the todo-list `Task`. The existing task create/list/get/update tools stay compatible.

Worktree isolation is implemented inside subagent execution. When `isolation: "worktree"` is requested, the subagent creates a git worktree under `.worktrees/subagents/<agent-id>`, runs with that directory as `cwd`, and records the worktree path on the `BackgroundTask`. If the current directory is not in a git repository or worktree creation fails, the tool returns a clear validation/runtime error.

## Data Model

learn-claude-code separates two concerns: **Plan/Todo tasks** (the work-tracking checklist: `pending → in_progress → completed`) and **Background execution tasks** (runtime lifecycle for async shells/agents: `pending → running → completed/failed/killed`). This design mirrors that separation instead of conflating the two.

### Plan Task Enhancements

The existing `Task` (used by `TaskCreate/TaskGet/TaskList/TaskUpdate`) is the todo-list model. Its statuses stay as-is: `Pending, InProgress, Completed, Deleted`.

Add optional fields to `Task` matching learn-claude-code V2:

- `owner: Option<String>` — who is assigned (used in teammate scenarios).
- `active_form: Option<String>` — present-continuous label shown in spinners (e.g. `"Running tests"`).
- `metadata: Option<HashMap<String, Value>>` — arbitrary key-value store for hooks/extensions.

`TaskUpdateTool` gains the ability to update `subject`, `description`, `owner`, `active_form`, merge `metadata`, and manage `add_blocks`/`add_blocked_by` dependencies (matching learn-claude-code's `TaskUpdateTool`). Status transitions stay the same.

### Background Task Model — new, separate from Plan Task

Background execution tasks represent running async work (subagents, background shells). They have their own struct, status enum, and persistence — independent of the todo-list `Task`.

**`BackgroundTaskStatus`** (new):
- `Pending` — created, not yet started.
- `Running` — executing in a Tokio task.
- `Completed` — finished successfully.
- `Failed` — terminated with error.
- `Killed` — cancelled via `task_stop`.

This matches learn-claude-code's `TaskStatus` (`pending, running, completed, failed, killed`) from `Task.ts`, not the todo-list status.

**`BackgroundTask`** (new):
- `id: String`
- `kind: String` — e.g. `"subagent"`, `"local_bash"`.
- `agent_id: Option<String>`
- `agent_type: Option<String>`
- `description: String`
- `status: BackgroundTaskStatus`
- `output: Option<String>` — final text (assistant response for agents, stdout+stderr for shells).
- `error: Option<String>`
- `worktree_path: Option<String>`
- `exit_code: Option<i32>` — for shell tasks.

`TaskOutputTool` and `TaskStopTool` operate on `BackgroundTask`, not on the todo-list `Task`. The existing todo-list tools (`TaskCreate` etc.) are unaffected.

## Subagent Definition Enhancements

Extend markdown frontmatter parsing with local fields inspired by `learn-claude-code`:

- `initialPrompt`: prepended to the first delegated prompt.
- `permissionMode`: accepted as metadata for hosts that later support per-agent permission policy.
- `skills`: string array, stored now and injected later when skill preloading is supported.
- `effort`: string metadata for model routing.

Fields not yet consumed by the runtime must be parsed, persisted in `AgentDefinition`, and documented as metadata-only. This avoids pretending we support full Claude Code semantics while making plugin/project agent definitions forward-compatible.

## Background Execution

`SubagentTool` behavior:

- Synchronous calls remain the default.
- `run_in_background: true` creates a `BackgroundTask` (not a todo-list `Task`), starts a child `AgentSession` in a Tokio task, and immediately returns:

```json
{
  "status": "async_launched",
  "agent_id": "agent_x",
  "task_id": "agent_x",
  "description": "Investigate parser bug",
  "agent_type": "Explore"
}
```

The background Tokio task:

- Uses a child cancellation state registered under the background task id.
- Emits progress to the parent progress channel when available.
- Updates `BackgroundTask` status to `Completed`, `Failed`, or `Killed`.
- Stores final assistant text in `BackgroundTask.output`.

## Stop And Output Tools

`task_output` input (matching learn-claude-code's `TaskOutputTool`):

```json
{
  "task_id": "agent_x",
  "block": true,
  "timeout": 30000
}
```

- `task_id`: the background task ID.
- `block` (default `true`): wait for completion when `true`, return current status immediately when `false`.
- `timeout` (default 30000ms, max 600000ms): max wait time when blocking.

Output includes `retrieval_status` (`success`, `timeout`, `not_ready`) plus the full `BackgroundTask` data: status, description, output, error, agent metadata, and worktree path when present.

`task_stop` input:

```json
{ "task_id": "agent_x", "shell_id": "deprecated_alias" }
```

Cancels the registered cancellation state. If the task has already finished, it returns the current status without error. Accepts `shell_id` as a deprecated alias for backward compatibility with `KillShell` tool references. Returns `message`, `task_id`, `task_type`, and optional `command` (description of stopped task).

## Delegation Prompt Guidance

Update subagent prompt guidance to teach:

- Delegate only self-contained tasks.
- Use multiple subagent calls in one assistant message for independent work.
- Give each subagent exact scope, files if known, constraints, and expected output.
- Avoid duplicate parent/subagent work.
- Use `run_in_background` for long-running work and `task_output` to inspect results.
- Use `isolation: "worktree"` only for write-heavy work that should not touch the parent checkout.

## Testing

Use TDD for each behavior:

- Plan `Task` persists new fields: `owner`, `active_form`, `metadata`.
- `TaskUpdateTool` updates subject, description, owner, active_form, add_blocks/add_blocked_by.
- `BackgroundTask` model persists the full lifecycle: `pending → running → completed/failed/killed`.
- `task_output` returns persisted output, metadata, and respects `block`/`timeout` params.
- `task_stop` cancels a running background task; no-op if already terminal.
- `run_in_background` returns immediately and later records completed output in a `BackgroundTask`.
- Killed background subagent records `Killed`.
- `isolation: "worktree"` changes child `cwd` and records `worktree_path` on `BackgroundTask`.
- Agent frontmatter parses new metadata fields: `initialPrompt`, `permissionMode`, `skills`, `effort`.
- Prompt text includes concrete delegation rules.

## Acceptance Criteria

- Existing synchronous subagent and fork tests still pass.
- Plan `Task` supports `owner`, `active_form`, `metadata` fields and `TaskUpdateTool` can update subject/description/blocks.
- `BackgroundTask` model is independent from plan `Task` with its own status lifecycle.
- Background subagent execution works with mock providers.
- `task_output` and `task_stop` tools are registered/exported and covered by tests.
- Worktree isolation is implemented and tested with a temporary git repository.
- Agent definitions parse the new metadata fields.
- Prompt guidance no longer advertises unsupported capabilities.
- Full `cargo test` passes.
