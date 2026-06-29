# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-06-29

### Added
- **Prompt system v2**: expanded `PromptAssembly` with built-in sections (`ToneStyleSection`, `TaskGuidanceSection`, `SafetySection`, `ToolUsageSection`), richer `IdentitySection`, and `PromptBlock` cache boundary support.
- **Default prompt assembly**: `default_coding_assembly()` helper, `AgentConfig::with_default_prompt_assembly()` builder, automatic fallback when no prompt is configured.
- **Tool prompts**: `Tool::prompt_text()` injects usage guidance into the system prompt; `ToolPromptsSection` renders all registered tool prompts.
- **System reminders**: `SystemReminder` enum with runtime injection of `<system-reminder>` messages after compaction and hook interception.
- **Bundled skills**: `explore` skill for deep codebase research; `AgentConfig::with_bundled_skills()` loads and exposes bundled skills.
- Release workflow: changelog generation, checksum calculation, desktop artifact upload.

### Changed
- Replaced hand-rolled `AnthropicProvider` and `OpenAIProvider` with `async-openai`-based `KimiProvider` and `DeepSeekProvider`.

### Fixed
- Desktop release: artifact glob paths, bundle output paths, nested artifact upload.

## [0.1.1] - 2026-06-28

### Added
- **Desktop app (Tauri)**: conversation session persistence across restarts, memory overview UI, settings management with API key configuration, workspace layout with agent rail and inspector toggle, TopBar and RunInspector components
- **Python TUI**: full Textual TUI with reactive AppState store, event loop for serve protocol, stream buffer for throttled markdown rendering, MessageBubble/ToolCard/HeaderWidget widgets, serve command with team module and plan mode
- **Subagent system**: background task execution, worktree isolation, task lifecycle output, stop tools, enriched guidance and definitions, enhanced status text and progress reporting
- **PowerShell support**: PowerShell parser and safety analysis, permission routing by shell kind, tool execution, inline approval rendering
- **Memory & context**: enhanced compaction strategy, memory injection with fingerprint tracking, improved relevance scoring with DeepSeek context sync, PromptProfile and SkillInjector for resizable SideWorkspace
- **Cost & billing**: token usage tracking with cost estimation, cache hit/miss pricing model, fuzzy model name resolution
- **CLI enhancements**: startup update check, internationalized homepage, improved input handling with history management, default shell configuration
- **Infrastructure**: git-cliff config for automated changelog, changelog-driven GitHub Release workflow, desktop release automation via GitHub Actions, Pages deployment

### Changed
- Merged runtime crate into core as frontend module (unified workspace)
- Flattened HistoryCell trait to ChatEntry enum, fixing 25+ bugs
- Compressed system prompt and tool definitions (~40% token reduction)
- Migrated Python TUI to reactive state architecture
- Optimized prompt layout for cache hits
- Replaced prompt starter quick-pick buttons with cleaner UI

### Fixed
- Broken pipe error when writing to stdin in CommandTool
- Stale `--locked` flag causing lock file conflicts in CI dry-runs
- Session files not deleted from disk on session reset
- ScrollArea overflow in desktop message list (flex-1 and min-h-full issues)
- Subagent tool calls inheriting parent's approval handler and permission engine
- Mouse capture enabling issues in TUI guard
- Protocol error handling on connection loss
- Hide console window warnings on non-Windows
- All 15 audit findings and final review findings (deadlock, scroll, approval, thinking)
- CI tag pattern and artifact glob consistency
- Cargo publish commands using correct manifest paths

## [0.1.0] - 2026-05-26

### Added
- Core agent runtime with streaming turn loop (`AgentSession`, `TurnEvent`, `TurnResult`)
- Provider abstraction with Kimi and DeepSeek backends (`ModelProvider` trait)
- Pluggable tool system with `Tool` trait and `ToolRegistry`
- Six built-in tools: Bash (shell), Read, Write, Edit, Glob, Grep
- Concurrent tool execution engine with batching
- Hook system for intercepting assistant messages (`PostSampling`, `Stop` phases)
- Context compaction: token-budget-aware summarization + per-message tool result truncation
- Rule-based permission engine with wildcard matching and command/cwd filtering
- JSONL session storage for save/resume (`JsonlStorage`)
- `SubagentTool`: nested agent session exposed as a tool
- Streaming tool progress via `ToolProgress` events
- Stale-write protection (file read tracking before writes)
- `MockProvider` for testing
- Integration test suite (22 tests)
- GitHub Actions CI (build + test on push/PR to main)

[Unreleased]: https://github.com/future-re/tiny_agent_core/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/future-re/tiny_agent_core/releases/tag/v0.1.2
[0.1.1]: https://github.com/future-re/tiny_agent_core/releases/tag/v0.1.1
[0.1.0]: https://github.com/future-re/tiny_agent_core/releases/tag/v0.1.0
