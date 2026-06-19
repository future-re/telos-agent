# Model Hint Routing — Thinking/Fast Model Separation

**Date**: 2026-06-19
**Status**: Design approved, pending implementation plan

## Motivation

The current agent uses a single model for all operations. DeepSeek offers two models with
distinct strengths:

| Model | Strength | Best for |
|---|---|---|
| `deepseek-v4-pro` | Deep reasoning, chain-of-thought | Planning, complex decisions, error recovery |
| `deepseek-v4-flash` | Fast, cheap | Tool execution, file operations, summarization |

Industry research shows ~83% of agent calls can be served by a fast model (LangChain Router,
2025). Dual-model architectures achieve equivalent quality at 1/5.5 the compute cost
(SAG, ICSE 2026).

## Design: Hint-Based Semantic Routing

### Core Idea

The turn loop annotates each provider request with a `ModelHint` describing the *semantic
nature* of the call (planning, execution, recovery, summarization). A `RoutedProvider`
resolves hints to concrete model names and delegates to the right backend. Providers that
don't understand hints ignore them — fully backward compatible.

### Architecture

```
AgentConfig
  └─ RoutedModelConfig { Thinking→"deepseek-v4-pro", Execution→"deepseek-v4-flash", ... }

AgentSession::run_turn_stream
  ├─ iteration=1                   → hint: Thinking
  ├─ processing tool results       → hint: Execution
  ├─ tool error                    → hint: Recovery
  └─ compaction/summarization      → hint: Summarization

RoutedProvider (implements ModelProvider)
  ├─ resolve hint → model name
  └─ delegate to cached DeepSeekProvider
```

### New Types

#### `ModelHint` (provider/types.rs)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelHint {
    Thinking,       // Strategic reasoning, planning, understanding intent
    Execution,      // Tool execution, simple operations, file reads
    Recovery,       // Error recovery, re-planning after failure
    Summarization,  // Conversation summarization, compaction
}
```

#### `CompletionRequest` extension

Add `model_hint: Option<ModelHint>` field. `None` means "use default" — backward compatible.

#### `RoutedModelConfig` (provider/routed.rs — new file)

```rust
pub struct RoutedModelConfig {
    pub routes: HashMap<ModelHint, String>,  // hint → model_name
    pub default_model: String,
    pub api_key: String,
    pub base_url: String,
}
```

Convenience constructor `RoutedModelConfig::dual(api_key, thinking, execution)` covers the
common two-model case, mapping Thinking+Recovery → thinking model, Execution+Summarization →
fast model.

#### `RoutedProvider` (provider/routed.rs — new file)

Implements `ModelProvider`. Pre-creates all `DeepSeekProvider` instances at construction time
(avoids lifetime issues with lazy initialization). `stream_complete` and `complete` resolve
the hint via config, then delegate.

### Routing Strategy (Turn Loop)

| Scenario | Condition | Hint |
|---|---|---|
| First iteration | `iteration == 1` | Thinking |
| Tool result processing | Previous turn had tool calls, no errors | Execution |
| Error recovery | Previous tool result `is_error == true` | Recovery |
| No-progress loop | ≥3 consecutive tool rounds with no effective output | Thinking |
| Compaction | `run_compaction_phase` internal | Summarization |
| Subagent/Fork | SubagentTool / ForkExecution | Execution |
| Fast path | `TaskPath::Fast` | Always Execution |

#### TaskPath Influence

- **Fast**: All calls use Execution — no thinking model needed.
- **Standard**: First call Thinking, rest Execution.
- **Heavy**: Re-evaluate with Thinking every 4 tool rounds, plus error recovery.

### Configuration

#### TOML

```toml
[agent]
provider = "deepseek"
model = "deepseek-v4-flash"           # fallback when no hints configured

[agent.models]
thinking = "deepseek-v4-pro"
fast = "deepseek-v4-flash"
```

#### CLI

```
--thinking-model deepseek-v4-pro
--fast-model deepseek-v4-flash
```

#### Compatibility

- Single model configured → creates plain `DeepSeekProvider`, no routing overhead
- Two different models → creates `RoutedProvider`, automatic routing
- No models configured → defaults to `deepseek-v4-flash` for everything

### Files Changed

| File | Change |
|---|---|
| `core/src/provider/types.rs` | Add `ModelHint` enum; add `model_hint` to `CompletionRequest` |
| `core/src/provider/routed.rs` | **New**: `RoutedModelConfig` + `RoutedProvider` |
| `core/src/provider/mod.rs` | Re-export routing types |
| `core/src/config.rs` | Add `model_routing: Option<RoutedModelConfig>` to `AgentConfig` |
| `core/src/runtime/session.rs` | Add `resolve_hint()`; pass hint through `call_provider()` |
| `core/src/compaction/strategy.rs` | Set `Summarization` hint in compaction requests |
| `cli/src/cli.rs` | Add `--thinking-model` / `--fast-model` flags |
| `cli/src/config.rs` | Dual-model config parsing; add `Routed` variant to `ResolvedProvider` |
| `cli/src/runner.rs` | Match `ResolvedProvider::Routed` |
| `cli/src/lib.rs` | Match new variant in `build_erased_provider` |

### Testing Strategy

- **Unit**: `RoutedModelConfig::resolve()` for each hint, missing hint → default
- **Unit**: `RoutedProvider` delegates to correct underlying provider
- **Integration**: Turn with thinking + execution hints → both providers called
- **Integration**: `TaskPath::Fast` → all calls use Execution
- **Regression**: Single model (no routing config) → behavior unchanged
- **Backward compat**: `CompletionRequest` without hint → uses default model

### Future Extensions

- Additional hints: `Vision`, `Code`, `Creative`
- Per-tool hint override: specific tools can request a specific hint
- Cost tracking: log which model served each call
- Dynamic routing: learn routing decisions from success/failure feedback
