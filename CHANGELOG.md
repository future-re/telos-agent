# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Expanded prompt system with new built-in sections (`ToneStyleSection`, `TaskGuidanceSection`, `SafetySection`, `ToolUsageSection`) and richer `IdentitySection` content adapted from publicly exposed Claude Code system prompts.
- `telos_agent::prompt::default_coding_assembly(tools, cwd, skills)` helper to build a standard coding-agent prompt assembly without manual section wiring; optionally includes a `SkillsSection`.
- `AgentConfig::with_default_prompt_assembly(tools)` builder method for one-line setup of the default prompt assembly.
- Automatic fallback: `AgentSession::run_turn` / `run_turn_stream` now builds the default prompt assembly when neither `prompt_assembly` nor `base_system_prompt` is configured.
- `examples/kimi_tool_loop.rs` now demonstrates `AgentConfig::with_default_prompt_assembly`.
- Tool-specific prompt guidance: `Tool::prompt_text()` lets each tool inject detailed usage instructions into the system prompt; core tools (`Bash`, `Read`, `Edit`, `Write`, `Glob`, `Grep`, `Subagent`, `Skill`, `WebSearch`, `WebFetch`, `AskUserQuestion`) now include adapted guidance.
- `ToolPromptsSection` renders all registered tool prompts under `## Tool-specific guidance` in the default assembly.
- `SystemReminder` enum and runtime injection of `<system-reminder>` user messages after compaction and hook interception.
- Prompt cache boundary: `PromptAssembly::build_blocks()` returns `Vec<PromptBlock>` with stability metadata; `CompletionRequest` gains `system_prompt_blocks` so future providers can apply per-block cache controls.
- Bundled `explore` skill for deep codebase research; `AgentConfig::with_bundled_skills()` loads bundled skills and exposes them through the default prompt assembly.

### Changed
- Replaced hand-rolled `AnthropicProvider` and `OpenAIProvider` with `async-openai`-based `KimiProvider` and `DeepSeekProvider`.

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

[Unreleased]: https://github.com/future-re/tiny_agent_core/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/future-re/tiny_agent_core/releases/tag/v0.1.0
