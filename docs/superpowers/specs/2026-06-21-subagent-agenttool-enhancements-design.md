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

Add a shared subagent runtime handle to `AgentConfig`, backed by the existing `TaskManager`. `SubagentTool` will use it when `run_in_background` is requested. Background subagents run in a Tokio task, update their task record as they progress, and write their final text or failure into the task output.

Expose two lifecycle tools:

- `task_output`: fetch one task's status and output.
- `task_stop`: request cancellation for a running task.

These tools intentionally operate on generic tasks, but the first implementation targets background subagent tasks. The existing task create/list/get/update tools stay compatible.

Worktree isolation is implemented inside subagent execution. When `isolation: "worktree"` is requested, the subagent creates a git worktree under `.worktrees/subagents/<agent-id>`, runs with that directory as `cwd`, and records the worktree path in task output/metadata. If the current directory is not in a git repository or worktree creation fails, the tool returns a clear validation/runtime error.

## Data Model

Extend `TaskStatus` with:

- `Failed`
- `Cancelled`

Extend `Task` with optional metadata:

- `kind`: string, for example `subagent`.
- `agent_id`
- `agent_type`
- `worktree_path`
- `error`

The existing `output: Option<String>` remains the canonical human-readable output. Metadata should be optional and backward compatible with existing task JSON.

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
- `run_in_background: true` creates a task, starts a child `AgentSession` in a Tokio task, and immediately returns:

```json
{
  "status": "async_launched",
  "agent_id": "agent_x",
  "task_id": "agent_x",
  "description": "Investigate parser bug",
  "agent_type": "Explore"
}
```

The background task:

- Uses a child cancellation state registered under the task id.
- Emits progress to the parent progress channel when available.
- Updates task status to `Completed`, `Failed`, or `Cancelled`.
- Stores final assistant text in `task.output`.

## Stop And Output Tools

`task_output` input:

```json
{ "task_id": "agent_x" }
```

Output includes status, description, output, error, agent metadata, and worktree path when present.

`task_stop` input:

```json
{ "task_id": "agent_x" }
```

It cancels the registered cancellation state. If the task has already finished, it returns the current status without error.

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

- Task model persists new statuses and metadata.
- `task_output` returns persisted output and metadata.
- `task_stop` marks or requests cancellation for a running task.
- `run_in_background` returns immediately and later records completed output.
- Cancelled background subagent records `Cancelled`.
- `isolation: "worktree"` changes child `cwd` and records path.
- Agent frontmatter parses new metadata fields.
- Prompt text includes concrete delegation rules.

## Acceptance Criteria

- Existing synchronous subagent and fork tests still pass.
- Background subagent execution works with mock providers.
- Task output and stop tools are registered/exported and covered by tests.
- Worktree isolation is implemented and tested with a temporary git repository.
- Agent definitions parse the new metadata fields.
- Prompt guidance no longer advertises unsupported capabilities.
- Full `cargo test` passes.
