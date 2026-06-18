# Default PromptAssembly Builder

## Goal

Make the new Claude Code-style prompt sections (Identity, ToneStyle, TaskGuidance, Safety, ToolUsage) usable out of the box, without requiring every caller to manually construct a `PromptAssembly`.

## Current State

- `PromptAssembly` and sections exist in `src/prompt/`.
- `examples/kimi_tool_loop.rs` manually assembles sections.
- If a user only sets `AgentConfig::base_system_prompt`, the new sections are not used.

## Design

1. Add a free function `telos_agent::prompt::default_coding_assembly(tools, cwd)` that returns a `PromptAssembly` pre-loaded with the standard sections:
   - `IdentitySection::new(None)`
   - `ToneStyleSection`
   - `TaskGuidanceSection`
   - `SafetySection`
   - `ToolUsageSection`
   - `ToolsSection(tools)`
   - `DateSection`
   - `CwdSection(cwd)`

2. Add `AgentConfig::with_default_prompt_assembly(self, tools) -> Result<Self, AgentError>` that builds the assembly and stores it in `prompt_assembly`, clearing `base_system_prompt`.

3. As a safety net, update `AgentSession::run_turn_stream` so that when **both** `prompt_assembly` and `base_system_prompt` are `None`, a full default assembly is built from the `ToolRegistry` passed to the turn. This avoids silent empty system prompts and ensures the model always sees the available tools.

## Interface

```rust
use std::sync::Arc;
use telos_agent::{AgentConfig, ToolRegistry};

let tools = Arc::new(ToolRegistry::new());
let config = AgentConfig::default()
    .with_default_prompt_assembly(tools)?;
```

## Error Handling

- `with_default_prompt_assembly` returns `Ok` unless the current directory cannot be resolved, which is unlikely.

## Testing

- Add an integration test verifying that `with_default_prompt_assembly` produces a system prompt containing key section markers ("Tone and style", "Doing tasks", "Executing actions with care", "Tool usage").
- Update existing prompt tests if necessary.

## Backwards Compatibility

- Existing callers that pass `base_system_prompt` or a custom `prompt_assembly` are unaffected.
- The fallback in `AgentSession::new` only triggers when both fields are `None`.
