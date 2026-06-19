# AgentTool Subagent System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a registry-backed AgentTool-style subagent system with typed agent definitions, plugin/project loading, `subagent_type` execution, tool filtering, and visible subagent progress.

**Architecture:** Add focused subagent modules for definitions, registry, built-ins, and run metadata. Keep `SubagentTool` as the public tool type while expanding its schema and execution path. Use existing `ToolProgress` and `ToolRegistry` primitives, extending context only where needed.

**Tech Stack:** Rust, Tokio, async-trait, serde/serde_json, serde_yaml via existing dependencies, telos-agent runtime and integration tests.

---

## File Structure

- Create `core/src/subagent/definition.rs`: agent definition structs, markdown frontmatter parser, model hint parsing.
- Create `core/src/subagent/registry.rs`: registry, override behavior, markdown directory loading, built-in registration.
- Create `core/src/subagent/builtins.rs`: built-in agent definitions.
- Modify `core/src/subagent/mod.rs`: wire registry into `SubagentTool`, support AgentTool schema, structured progress, filtered tools.
- Modify `core/src/subagent/fork.rs`: keep existing fork behavior compatible.
- Modify `core/src/tool/mod.rs`: add parent `tool_call_id` to `ToolContext`.
- Modify `core/src/executor/stream.rs` and `core/src/executor/sync.rs`: populate `ToolContext.tool_call_id`.
- Modify `core/src/plugin/registry/apply.rs`: add an overload or new method that applies plugin agents into `SubagentRegistry`.
- Modify `core/src/lib.rs`: export new subagent registry/definition types.
- Modify `core/src/prompt/builtins.rs`: align guidance with actual `subagent_type` support.
- Modify `cli/src/tui/app.rs` only if needed after core progress IDs are fixed.
- Modify `core/tests/integration_tests.rs` and core unit tests for coverage.

## Tasks

### Task 1: Agent Definition Parser

**Files:**
- Create: `core/src/subagent/definition.rs`
- Modify: `core/src/subagent/mod.rs`

- [ ] Write failing unit tests for parsing valid markdown, missing name, missing description, tool lists, model hints, max iterations, background, and isolation.
- [ ] Run `cargo test -p telos-agent subagent::definition` and confirm failures are due to missing module/types.
- [ ] Implement `AgentDefinition`, `AgentSource`, `AgentIsolation`, `AgentDefinition::from_markdown`.
- [ ] Run targeted tests and confirm pass.

### Task 2: Subagent Registry And Built-ins

**Files:**
- Create: `core/src/subagent/registry.rs`
- Create: `core/src/subagent/builtins.rs`
- Modify: `core/src/subagent/mod.rs`
- Modify: `core/src/lib.rs`

- [ ] Write failing tests for registry override order, lookup, rendered listing, directory loading, and built-ins.
- [ ] Run targeted tests and confirm failures.
- [ ] Implement `SubagentRegistry`, built-in agents, and exports.
- [ ] Run targeted tests and confirm pass.

### Task 3: ToolContext Parent Tool Call Id

**Files:**
- Modify: `core/src/tool/mod.rs`
- Modify: `core/src/executor/stream.rs`
- Modify: `core/src/executor/sync.rs`
- Modify tests in `core/tests/integration_tests.rs`

- [ ] Write failing integration test proving a progress event emitted by a tool carries the parent `tool_call_id`.
- [ ] Run the test and confirm it fails because the id is missing.
- [ ] Add `tool_call_id: Option<String>` to `ToolContext` and populate it in both executors.
- [ ] Run targeted tests and confirm pass.

### Task 4: Registry-Backed SubagentTool Schema And Validation

**Files:**
- Modify: `core/src/subagent/mod.rs`
- Modify: `core/tests/integration_tests.rs`

- [ ] Write failing tests for `subagent_type` accepted by schema/validation and unknown types producing a helpful validation error.
- [ ] Run targeted tests and confirm failures.
- [ ] Add a registry field to `SubagentTool`, preserve `SubagentTool::new`, add `SubagentTool::with_registry`, update schema with `description`, `subagent_type`, `model`, `run_in_background`, `isolation`, and compatibility fields.
- [ ] Run targeted tests and confirm pass.

### Task 5: Agent Execution, Prompt Selection, And Result Metadata

**Files:**
- Modify: `core/src/subagent/mod.rs`
- Modify: `core/tests/integration_tests.rs`

- [ ] Write failing test where `subagent_type: "Explore"` makes the inner provider receive the Explore system prompt.
- [ ] Write failing test where result JSON contains `agent_id`, `agent_type`, `description`, `status`, `final_text`, and `event_count`.
- [ ] Implement agent resolution, child config creation, prompt override, max iteration override, model hint override where supported by config/runtime, and structured result.
- [ ] Run targeted tests and confirm pass.

### Task 6: Tool Filtering

**Files:**
- Modify: `core/src/subagent/mod.rs`
- Add unit tests near subagent code.

- [ ] Write failing tests for allowlist and denylist filtering.
- [ ] Run targeted tests and confirm failures.
- [ ] Implement filtered child `ToolRegistry` construction using canonical definitions and aliases.
- [ ] Run targeted tests and confirm pass.

### Task 7: Plugin Agent Loading

**Files:**
- Modify: `core/src/plugin/registry/apply.rs`
- Modify: `core/src/plugin/registry/tests.rs` or `core/tests/integration_tests.rs`
- Modify: `core/src/lib.rs`

- [ ] Write failing test where a plugin `agents` markdown file is applied into `SubagentRegistry` with plugin namespace.
- [ ] Run targeted tests and confirm failure.
- [ ] Add `PluginRegistry::apply_subagents(&self, subagents: &mut SubagentRegistry)` or extend apply without breaking existing callers.
- [ ] Run targeted tests and confirm pass.

### Task 8: Subagent Progress Visibility

**Files:**
- Modify: `core/src/subagent/mod.rs`
- Modify: `core/tests/integration_tests.rs`

- [ ] Write failing streaming integration test where subagent progress is attached to parent `tool_call_id` and contains structured `data.kind == "subagent"`.
- [ ] Run targeted test and confirm failure.
- [ ] Emit structured progress for provider request, child tool call, and finish.
- [ ] Run targeted test and confirm pass.

### Task 9: Prompt Guidance And Compatibility

**Files:**
- Modify: `core/src/subagent/mod.rs`
- Modify: `core/src/prompt/builtins.rs`
- Modify: existing integration tests if expectations changed.

- [ ] Write failing test that default prompt/tool prompts mention supported `subagent_type` usage and available built-ins.
- [ ] Run targeted test and confirm failure.
- [ ] Update prompt guidance to match new AgentTool schema.
- [ ] Ensure existing prompt-only and fork-mode subagent integration tests still pass.

### Task 10: Full Verification

**Files:**
- All touched files.

- [ ] Run `cargo fmt`.
- [ ] Run `cargo test`.
- [ ] Fix failures with additional red/green cycles as needed.
- [ ] Commit implementation with a concise message.
