# Core Organization Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure `core` so large modules have focused responsibilities while preserving current public APIs and behavior.

**Architecture:** This is a conservative refactor. Public re-exports from `telos_agent` and `core::tools` remain stable while large files are split into private submodules. Each phase is behavior-preserving and verified before moving to the next.

**Tech Stack:** Rust 2024, Cargo workspace, tokio, serde_json, async-trait.

---

### Task 1: Baseline Verification

**Files:**
- Read: `core/src/tools/browser.rs`
- Read: `core/src/runtime/session.rs`
- Read: `core/tests/integration_tests.rs`

- [ ] **Step 1: Confirm working tree state**

Run: `git status --short`
Expected: no unrelated tracked changes before refactoring begins.

- [ ] **Step 2: Run compiler baseline**

Run: `cargo check -p telos_agent`
Expected: command exits with status 0.

- [ ] **Step 3: Run test baseline**

Run: `cargo test -p telos_agent`
Expected: command exits with status 0. If this is too slow or fails because of environment-only integration tests, record the exact failing tests before continuing.

### Task 2: Split Browser Tool Module

**Files:**
- Modify: `core/src/tools/browser.rs`
- Create: `core/src/tools/browser/mod.rs`
- Create: `core/src/tools/browser/manager.rs`
- Create: `core/src/tools/browser/session.rs`
- Create: `core/src/tools/browser/cdp.rs`
- Create: `core/src/tools/browser/tool_impls.rs`
- Create: `core/src/tools/browser/scripts.rs`
- Create: `core/src/tools/browser/util.rs`
- Modify: `core/src/tools/mod.rs`

- [ ] **Step 1: Move the current file into a module directory without behavior changes**

Move `core/src/tools/browser.rs` to `core/src/tools/browser/mod.rs`. Update `core/src/tools/mod.rs` only if required by Rust module resolution.

- [ ] **Step 2: Verify the move**

Run: `cargo check -p telos_agent`
Expected: command exits with status 0.

- [ ] **Step 3: Extract CDP client**

Move `CdpClient` and websocket-specific imports into `core/src/tools/browser/cdp.rs`. Re-export it inside `browser/mod.rs` as `pub(super)` or `pub(crate)` only as needed by `session.rs`.

- [ ] **Step 4: Extract scripts**

Move `PAGE_SUMMARY_SCRIPT`, `BROWSER_STATE_SCRIPT`, `BROWSER_CLICK_SCRIPT`, `BROWSER_TYPE_SCRIPT`, `BROWSER_SELECT_SCRIPT`, and `BROWSER_ACTION_HELPERS` into `core/src/tools/browser/scripts.rs`.

- [ ] **Step 5: Extract utilities**

Move browser argument parsing, URL validation, browser path lookup, safe path helpers, bookmark helpers, and progress helpers into `core/src/tools/browser/util.rs`.

- [ ] **Step 6: Extract manager and session**

Move `BrowserManager`, `BrowserSession`, `Viewport`, and `SharedSession` into focused files. Keep session methods in `session.rs`; keep session map lifecycle in `manager.rs`.

- [ ] **Step 7: Extract tool wrappers**

Move `BrowserStartTool`, `BrowserNavigateTool`, `BrowserStateTool`, `BrowserClickTool`, `BrowserTypeTool`, `BrowserSelectTool`, `BrowserScrollTool`, `BrowserBackTool`, `BrowserScreenshotTool`, `BrowserCloseTool`, and `BrowserFindUrlTool` into `tool_impls.rs`.

- [ ] **Step 8: Verify browser split**

Run: `cargo test -p telos_agent browser --lib`
Expected: browser unit tests pass.

- [ ] **Step 9: Verify crate**

Run: `cargo check -p telos_agent`
Expected: command exits with status 0.

### Task 3: Split Integration Tests

**Files:**
- Modify: `core/tests/integration_tests.rs`
- Create: `core/tests/runtime_tests.rs`
- Create: `core/tests/tool_tests.rs`
- Create: `core/tests/storage_tests.rs`
- Create: `core/tests/skill_prompt_tests.rs`
- Create: `core/tests/memory_tests.rs`
- Create: `core/tests/subagent_plugin_tests.rs`

- [ ] **Step 1: Identify shared test fixtures**

Move reusable provider/tool fixtures into a small shared module inside each test file only where needed. Avoid a large shared helper file unless duplication becomes meaningful.

- [ ] **Step 2: Move session/runtime tests**

Move turn loop, cancellation, hooks, compaction, token budget, and default prompt assembly session tests into `runtime_tests.rs`.

- [ ] **Step 3: Move tool tests**

Move filesystem, shell, approval, web, ask-user-question, tool registry, and tool prompt tests into `tool_tests.rs`.

- [ ] **Step 4: Move storage tests**

Move JSONL storage and session save/resume tests into `storage_tests.rs`.

- [ ] **Step 5: Move skills and prompt tests**

Move skill loader/registry and prompt section tests into `skill_prompt_tests.rs`.

- [ ] **Step 6: Move memory tests**

Move memory tool and memory prompt section tests into `memory_tests.rs`.

- [ ] **Step 7: Move subagent and plugin tests**

Move subagent and plugin integration tests into `subagent_plugin_tests.rs`.

- [ ] **Step 8: Verify test split**

Run: `cargo test -p telos_agent --tests`
Expected: integration tests pass with the same behavior as before.

### Task 4: Split Runtime Session Internals

**Files:**
- Modify: `core/src/runtime/session.rs`
- Create: `core/src/runtime/provider_call.rs`
- Create: `core/src/runtime/compaction_phase.rs`
- Create: `core/src/runtime/hook_phase.rs`
- Create: `core/src/runtime/tool_phase.rs`
- Create: `core/src/runtime/persistence.rs`
- Modify: `core/src/runtime/mod.rs`

- [ ] **Step 1: Extract provider stream aggregation**

Move provider retry and stream-to-message aggregation out of `session.rs` while preserving `TurnEvent` emission and retry metrics.

- [ ] **Step 2: Extract compaction phase**

Move token budget and char budget compaction phase into `compaction_phase.rs`.

- [ ] **Step 3: Extract hook phase**

Move hook execution and hook reminder behavior into `hook_phase.rs`.

- [ ] **Step 4: Extract tool execution phase**

Move tool execution event mapping, tool result compaction, and tool metrics updates into `tool_phase.rs`.

- [ ] **Step 5: Extract persistence helpers**

Move session metadata save/resume support into `persistence.rs` if it can be done without exposing internals broadly.

- [ ] **Step 6: Verify runtime split**

Run: `cargo test -p telos_agent runtime`
Expected: runtime unit tests pass.

- [ ] **Step 7: Verify crate**

Run: `cargo test -p telos_agent`
Expected: command exits with status 0.

### Task 5: Audit Remaining Medium Modules

**Files:**
- Inspect: `core/src/memory/index.rs`
- Inspect: `core/src/tools/memory.rs`
- Inspect: `core/src/tools/web_search.rs`
- Inspect: `core/src/prompt/builtins.rs`
- Inspect: `core/src/config.rs`
- Inspect: `core/src/mcp/client.rs`

- [ ] **Step 1: Recompute large-file list**

Run: `wc -l $(rg --files core/src core/tests | sort) | sort -nr | head -40`
Expected: no single non-test implementation file remains above roughly 700 lines unless it is a generated or schema file.

- [ ] **Step 2: Split memory modules if still oversized**

Move memory store persistence, query sorting, maintenance policy, and index rendering into separate files if `memory/index.rs` remains hard to scan.

- [ ] **Step 3: Split web search if still oversized**

Separate tool wrapper, providers, parsers, filters, and transport if `tools/web_search.rs` remains hard to scan.

- [ ] **Step 4: Split prompt builtins if still oversized**

Move major prompt sections into themed files if `prompt/builtins.rs` remains hard to scan.

- [ ] **Step 5: Final verification**

Run: `cargo test -p telos_agent`
Expected: command exits with status 0.

- [ ] **Step 6: Final organization audit**

Run: `wc -l $(rg --files core/src core/tests | sort) | sort -nr | head -40`
Expected: remaining large files have a clear reason to stay large, and module boundaries are easy to explain.
