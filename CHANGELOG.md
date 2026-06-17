# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/tiny-agent/tiny_agent_core/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tiny-agent/tiny_agent_core/releases/tag/v0.1.0
