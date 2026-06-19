# AgentTool Subagent System Design

## Goal

Replace the current prompt-only `SubagentTool` with a Claude Code-style AgentTool system: typed agent definitions, registry-backed `subagent_type` selection, plugin/project agent loading, per-agent tool restrictions, observable subagent execution, and a compatibility path for existing `subagent` calls.

## Reference

The reference implementation is `/home/alin/codework/learn-claude-code`, especially:

- `tools/AgentTool/loadAgentsDir.ts` for agent definition loading and override behavior.
- `tools/AgentTool/AgentTool.tsx` for model-facing schema and execution routing.
- `tools/AgentTool/runAgent.ts` for subagent identity, sidechain metadata, progress, and lifecycle.
- `tools/AgentTool/prompt.ts` for agent usage guidance and prompt-cache-conscious agent listing.
- `utils/plugins/loadPluginAgents.ts` for plugin agent markdown loading and namespacing.

## Current State

`telos-agent` already has:

- `SubagentTool`, which can run a nested `AgentSession` or fork lenses.
- `TurnEvent::ToolProgress`, consumed by the CLI/TUI.
- Plugin manifests with an `agents` field.
- Prompt guidance that tells the model to use `subagent_type Explore`.

The gaps are:

- `SubagentTool` does not accept `subagent_type`.
- There is no `SubagentRegistry` or `AgentDefinition`.
- Plugin `agents` are discovered but not applied.
- Subagent progress is coarse and uses `tool_call_id: None`, so the TUI cannot reliably attach it to the parent tool call.
- Agent execution has no durable agent identity, metadata, or transcript surface.

## Architecture

### Agent Definitions

Add `core/src/subagent/definition.rs` with:

- `AgentDefinition`: `name`, `description`, `system_prompt`, `allowed_tools`, `disallowed_tools`, `model_hint`, `max_iterations`, `background`, `isolation`, `source`, and optional metadata.
- `AgentSource`: `BuiltIn`, `Project`, `Plugin`, `User`.
- `AgentIsolation`: initially `None` and `Worktree`. Remote isolation is reserved for a later CLI/server integration.

Supported markdown frontmatter:

```yaml
---
name: Explore
description: Use this agent for broad read-only codebase exploration.
tools: [Read, Grep, Glob, Bash]
disallowedTools: [Write, Edit]
model: execution
maxIterations: 8
background: false
isolation: none
---
System prompt body.
```

The first implementation accepts `model` values that map to existing `ModelHint`: `thinking`, `execution`, `recovery`, and `summarization`.

### Agent Registry

Add `core/src/subagent/registry.rs` with:

- `SubagentRegistry::new()`.
- `register(definition)`, where later registrations override earlier definitions by name.
- `get(name)`, `definitions()`, and `render_listing()`.
- `with_builtin_agents()`.
- `load_markdown_file(path, source)` and `load_markdown_dir(dir, source)`.

Override priority follows the reference shape:

1. Built-in agents.
2. Plugin agents.
3. Project/user agents.

The core crate will expose registry primitives. CLI wiring for user-global dirs can come after core behavior is stable.

### Built-in Agents

Add built-ins:

- `general-purpose`: full tool access, general autonomous work.
- `Explore`: read-only exploration using Read/Grep/Glob/Web tools and safe shell inspection.
- `Plan`: read-only exploration and planning.
- `Verification`: validation/testing focused agent.

Built-ins should be usable without any filesystem config.

### Plugin Agent Loading

Implement plugin application for `agents`:

- Parse resolved plugin markdown files into `AgentDefinition`.
- Namespace plugin agents as `plugin_name:agent_name`.
- Ignore dangerous per-agent escalation fields that core does not support.
- Register definitions into the configured `SubagentRegistry`.

This integrates with existing plugin discovery and `PluginRegistry::apply`.

### AgentTool-Compatible Subagent Tool

Keep the public Rust type name `SubagentTool` for compatibility, but update the model-facing schema to support AgentTool input:

- `description`: short task summary.
- `prompt`: task for the agent.
- `subagent_type`: optional specialized agent name; defaults to `general-purpose`.
- `model`: optional model hint override.
- `run_in_background`: accepted but initially returns a clear unsupported error unless task persistence support is explicitly wired in this implementation.
- `isolation`: `none` or `worktree`; worktree is accepted only when implemented by CLI/session config. Core initially rejects it with a structured unsupported error.
- Existing `system_prompt`, `max_iterations`, `mode`, and `forks` remain as compatibility fields.

Agent mode behavior:

1. Resolve `subagent_type` against the registry.
2. Build a child `AgentConfig` from the parent config.
3. Apply agent system prompt, max iterations, model hint, and tool allow/deny filtering.
4. Run `AgentSession::run_turn_stream`.
5. Return JSON containing `agent_id`, `agent_type`, `description`, `final_text`, `event_count`, and `status`.

Fork mode remains available for compatibility, but should become a named execution strategy rather than the only multi-agent abstraction.

### Tool Filtering

Add a filtered view of `ToolRegistry` for a child agent:

- If `allowed_tools` is empty, all tools are available unless denied.
- `allowed_tools` supports exact canonical names and aliases.
- `disallowed_tools` removes tools after allowlist filtering.
- The child should not receive `subagent` by default when the selected agent is read-only unless explicitly allowed, preventing accidental recursive fanout.

### Visibility

Change subagent progress emission to include the parent tool call id when available:

- Extend `ToolContext` with `tool_call_id: Option<String>`.
- Set it in executor before invoking the tool.
- `SubagentTool` forwards progress using that id.

Add structured `ToolProgress.data` payloads:

```json
{
  "kind": "subagent",
  "agent_id": "...",
  "agent_type": "Explore",
  "event": "tool_call",
  "name": "Grep"
}
```

The TUI can keep its existing rendering but will now attach progress to the parent tool card. A richer nested view can be layered on top later without changing core events again.

### Metadata And Transcript

Core should expose enough data for transcript persistence without forcing a storage layout:

- `SubagentRunMetadata`: `agent_id`, `agent_type`, `description`, `parent_session_id`, `parent_turn_id`, start/end timestamps, status.
- `SubagentRunResult`: metadata plus final text and event count.

The initial implementation returns this in tool JSON and streams structured progress. Full sidechain JSONL persistence can be added after core API shape stabilizes.

### Prompt Guidance

Update tool prompt guidance and default tool usage section:

- Stop mentioning unsupported `subagent_type Explore` until it is actually supported.
- List available agents from the registry in `SubagentTool::prompt_text()`.
- Keep guidance concise and cache-conscious. Dynamic listings should come from the registry at tool construction time, not from constantly changing tool schemas.

### Testing

Required coverage:

- Markdown parsing accepts valid frontmatter and rejects missing name/description.
- Registry override order works.
- Built-in agents are registered.
- Plugin `agents` are applied into a registry.
- `SubagentTool` accepts `subagent_type` and applies the agent prompt.
- Unknown `subagent_type` returns a validation error listing available agents.
- Tool filtering removes disallowed tools.
- Subagent progress carries the parent tool call id.
- Existing prompt-only subagent and fork integration tests still pass.

## Non-Goals For This Pass

- Remote agent execution.
- A full background worker daemon.
- Interactive SendMessage-to-running-agent routing.
- Full sidechain transcript browser.

The design keeps room for these features but avoids pretending they are implemented before the core contract is stable.
