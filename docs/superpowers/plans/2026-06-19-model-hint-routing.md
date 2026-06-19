# Model Hint Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable automatic routing between thinking (deepseek-v4-pro) and fast (deepseek-v4-flash) models based on the semantic nature of each provider call.

**Architecture:** Add a `ModelHint` enum carried on `CompletionRequest`. Turn loop annotates requests with hints (Thinking/Execution/Recovery/Summarization). New `RoutedProvider` delegates to cached `DeepSeekProvider` instances keyed by resolved model name. Backward compatible — single model users see no change.

**Tech Stack:** Rust, tokio, async_trait, async_openai (existing stack)

## Global Constraints

- Backward compatible: single-model config must behave identically to current
- `Option<ModelHint>` default = `None` means "use default model"
- `RoutedProvider` pre-creates all providers at construction time (no lazy init)
- API key shared across all routed models
- CLI flags: `--thinking-model`, `--fast-model`; TOML: `[agent.models]` section

---

### Task 1: Core Types — ModelHint + CompletionRequest

**Files:**
- Modify: `core/src/provider/types.rs:1-67`
- Modify (add `model_hint: None`): `core/src/provider/test.rs:36`
- Modify (add `model_hint: None`): `core/src/hooks/prompt.rs:11`
- Modify (add `model_hint: None`): `core/src/provider/deepseek.rs:148,180`
- Modify (add `model_hint: None`): `core/src/subagent/fork.rs:162`

**Interfaces:**
- Produces: `ModelHint` enum (Thinking, Execution, Recovery, Summarization)
- Produces: `CompletionRequest.model_hint: Option<ModelHint>` field

- [ ] **Step 1: Add ModelHint enum and extend CompletionRequest**

In `core/src/provider/types.rs`, add before `CompletionRequest`:

```rust
/// Semantic routing hint — describes the nature of a provider call so that
/// a routing provider can select an appropriate model.
///
/// Providers that don't support routing ignore this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelHint {
    /// Strategic reasoning: understanding user intent, planning, complex decisions.
    Thinking,
    /// Tool execution: processing tool results, simple file operations, retrieval.
    Execution,
    /// Error recovery: re-evaluating and re-planning after a tool failure.
    Recovery,
    /// Summarization: conversation compaction, history compression.
    Summarization,
}
```

In the same file, add `model_hint` to `CompletionRequest`:

```rust
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub system_prompt: Option<String>,
    pub system_prompt_blocks: Option<Vec<PromptBlock>>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    /// Optional model routing hint. When `None`, the provider uses its default model.
    /// When `Some`, a routing-aware provider may select a different model.
    pub model_hint: Option<ModelHint>,
}
```

- [ ] **Step 2: Add unit test for ModelHint + CompletionRequest defaults**

In `core/src/provider/types.rs`, add a test module at the bottom (before any existing `#[cfg(test)]` or at file end):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_hint_is_none_by_default() {
        let req = CompletionRequest {
            system_prompt: None,
            system_prompt_blocks: None,
            messages: vec![],
            tools: vec![],
            model_hint: None,
        };
        assert!(req.model_hint.is_none());
    }

    #[test]
    fn model_hint_can_be_set() {
        let req = CompletionRequest {
            system_prompt: None,
            system_prompt_blocks: None,
            messages: vec![],
            tools: vec![],
            model_hint: Some(ModelHint::Thinking),
        };
        assert_eq!(req.model_hint, Some(ModelHint::Thinking));
    }

    #[test]
    fn model_hint_is_copy_and_eq() {
        let a = ModelHint::Thinking;
        let b = a;
        assert_eq!(a, b);
    }
}
```

- [ ] **Step 3: Add model_hint: None to all existing CompletionRequest construction sites**

In `core/src/provider/test.rs:35-42`:
```rust
fn simple_request() -> CompletionRequest {
    CompletionRequest {
        system_prompt: Some("Reply in one short sentence.".into()),
        system_prompt_blocks: None,
        messages: vec![Message::user("What is the capital of France?")],
        tools: vec![],
        model_hint: None,
    }
}
```

In `core/src/hooks/prompt.rs:11`, add `model_hint: None,` to the struct literal.

In `core/src/provider/deepseek.rs:148` (inside `completes_chat_request` test):
```rust
let request = CompletionRequest {
    system_prompt: None,
    system_prompt_blocks: None,
    messages: vec![Message::user("Hi")],
    tools: vec![],
    model_hint: None,
};
```

In `core/src/provider/deepseek.rs:180` (inside `streams_chat_response` test):
```rust
let request = CompletionRequest {
    system_prompt: None,
    system_prompt_blocks: None,
    messages: vec![Message::user("Hi")],
    tools: vec![],
    model_hint: None,
};
```

In `core/src/subagent/fork.rs:162`, add `model_hint: None,` to the struct literal.

- [ ] **Step 4: Run tests to verify compilation and existing tests pass**

```bash
cargo test -p telos_agent
```
Expected: all existing tests pass (the new field defaults to None, no behavior change).

- [ ] **Step 5: Commit**

```bash
git add core/src/provider/types.rs core/src/provider/test.rs core/src/hooks/prompt.rs core/src/provider/deepseek.rs core/src/subagent/fork.rs
git commit -m "feat: add ModelHint enum and model_hint field to CompletionRequest

Adds ModelHint::{Thinking, Execution, Recovery, Summarization} for
semantic routing. CompletionRequest.model_hint defaults to None for
backward compatibility.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: RoutedModelConfig + RoutedProvider

**Files:**
- Create: `core/src/provider/routed.rs`

**Interfaces:**
- Consumes: `ModelHint`, `CompletionRequest` (from Task 1)
- Produces: `RoutedModelConfig` struct with `resolve(&self, hint: Option<ModelHint>) -> &str` and `dual(api_key, thinking, execution) -> Self`
- Produces: `RoutedProvider` struct implementing `ModelProvider`

- [ ] **Step 1: Create core/src/provider/routed.rs with full implementation**

```rust
//! Hint-based model routing provider.
//!
//! [`RoutedModelConfig`] maps [`ModelHint`] values to concrete model names.
//! [`RoutedProvider`] implements [`ModelProvider`] by resolving the hint on
//! each request and delegating to a pre-created [`DeepSeekProvider`].

use std::collections::{HashMap, HashSet};
use std::pin::Pin;

use async_trait::async_trait;
use futures_core::stream::Stream;

use crate::error::AgentError;
use crate::provider::deepseek::{DeepSeekConfig, DeepSeekProvider};
use crate::provider::types::{CompletionRequest, CompletionResponse, ModelHint, ProviderEvent};
use crate::provider::ModelProvider;

/// Maps [`ModelHint`] values to concrete model names.
///
/// Hints not present in the map fall back to `default_model`.
#[derive(Debug, Clone)]
pub struct RoutedModelConfig {
    /// hint → model_name mapping
    pub routes: HashMap<ModelHint, String>,
    /// Model used when no hint matches or hint is `None`
    pub default_model: String,
    /// API key shared across all routed models
    pub api_key: String,
    /// Base URL shared across all routed models
    pub base_url: String,
}

impl RoutedModelConfig {
    /// Resolve a hint to a concrete model name.
    /// Returns `default_model` when hint is `None` or not in the routes map.
    pub fn resolve(&self, hint: Option<ModelHint>) -> &str {
        hint.and_then(|h| self.routes.get(&h).map(|s| s.as_str()))
            .unwrap_or(&self.default_model)
    }

    /// Convenience constructor for the common two-model case.
    ///
    /// Routes Thinking + Recovery → `thinking`, Execution + Summarization → `execution`.
    /// Default model = `execution` (fast path).
    pub fn dual(api_key: String, thinking: String, execution: String) -> Self {
        let mut routes = HashMap::new();
        routes.insert(ModelHint::Thinking, thinking.clone());
        routes.insert(ModelHint::Recovery, thinking);
        routes.insert(ModelHint::Execution, execution.clone());
        routes.insert(ModelHint::Summarization, execution.clone());
        Self {
            routes,
            default_model: execution,
            api_key,
            base_url: "https://api.deepseek.com".into(),
        }
    }

    /// Collect all unique model names referenced in this config.
    fn all_models(&self) -> HashSet<&str> {
        let mut models: HashSet<&str> = self.routes.values().map(|s| s.as_str()).collect();
        models.insert(&self.default_model);
        models
    }
}

/// A [`ModelProvider`] that routes requests to different models based on
/// [`ModelHint`](crate::provider::ModelHint).
///
/// Providers are pre-created at construction time — one per unique model name
/// in the config. Provider selection is a simple HashMap lookup with no
/// allocation on the hot path.
pub struct RoutedProvider {
    config: RoutedModelConfig,
    /// model_name → provider (pre-created)
    providers: HashMap<String, DeepSeekProvider>,
}

impl RoutedProvider {
    pub fn new(config: RoutedModelConfig) -> Self {
        let mut providers = HashMap::new();
        for model in config.all_models() {
            let provider_config = DeepSeekConfig {
                api_key: config.api_key.clone(),
                model: model.to_string(),
                base_url: config.base_url.clone(),
            };
            providers.insert(model.to_string(), DeepSeekProvider::new(provider_config));
        }
        Self { config, providers }
    }

    /// Look up the provider for a given hint.
    fn resolve(&self, hint: Option<ModelHint>) -> &DeepSeekProvider {
        let model = self.config.resolve(hint);
        // Safety: new() pre-creates providers for every model in config
        &self.providers[model]
    }
}

#[async_trait]
impl ModelProvider for RoutedProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AgentError> {
        let provider = self.resolve(request.model_hint);
        provider.complete(request).await
    }

    fn stream_complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<ProviderEvent, AgentError>> + Send + 'a>> {
        let provider = self.resolve(request.model_hint);
        // provider borrows from self.providers (lifetime = 'a) ✅
        provider.stream_complete(request)
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        self.resolve(None).estimate_tokens(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_config() -> RoutedModelConfig {
        let mut routes = HashMap::new();
        routes.insert(ModelHint::Thinking, "deepseek-v4-pro".into());
        routes.insert(ModelHint::Execution, "deepseek-v4-flash".into());
        routes.insert(ModelHint::Recovery, "deepseek-v4-pro".into());
        routes.insert(ModelHint::Summarization, "deepseek-v4-flash".into());
        RoutedModelConfig {
            routes,
            default_model: "deepseek-v4-flash".into(),
            api_key: "test-key".into(),
            base_url: "https://api.deepseek.com".into(),
        }
    }

    #[test]
    fn resolve_known_hint_returns_correct_model() {
        let config = test_config();
        assert_eq!(config.resolve(Some(ModelHint::Thinking)), "deepseek-v4-pro");
        assert_eq!(config.resolve(Some(ModelHint::Execution)), "deepseek-v4-flash");
    }

    #[test]
    fn resolve_none_returns_default() {
        let config = test_config();
        assert_eq!(config.resolve(None), "deepseek-v4-flash");
    }

    #[test]
    fn resolve_unmapped_hint_returns_default() {
        // Re-use a hint not in routes — simulating future hint
        let config = test_config();
        // ModelHint doesn't have other variants yet, so None tests the fallback.
        assert_eq!(config.resolve(None), "deepseek-v4-flash");
    }

    #[test]
    fn dual_constructor_maps_correctly() {
        let config = RoutedModelConfig::dual(
            "key".into(),
            "pro-model".into(),
            "flash-model".into(),
        );
        assert_eq!(config.resolve(Some(ModelHint::Thinking)), "pro-model");
        assert_eq!(config.resolve(Some(ModelHint::Recovery)), "pro-model");
        assert_eq!(config.resolve(Some(ModelHint::Execution)), "flash-model");
        assert_eq!(config.resolve(Some(ModelHint::Summarization)), "flash-model");
        assert_eq!(config.resolve(None), "flash-model");
    }

    #[test]
    fn all_models_collects_unique_names() {
        let config = test_config();
        let models = config.all_models();
        assert_eq!(models.len(), 2);
        assert!(models.contains("deepseek-v4-pro"));
        assert!(models.contains("deepseek-v4-flash"));
    }

    #[test]
    fn routed_provider_constructs_without_error() {
        let config = test_config();
        let provider = RoutedProvider::new(config);
        // Just verify construction succeeds and providers map is populated
        assert_eq!(provider.providers.len(), 2);
    }

    #[test]
    fn estimate_tokens_delegates_to_default() {
        let config = test_config();
        let provider = RoutedProvider::new(config);
        // estimate_tokens uses the default provider; actual value depends on
        // tiktoken-rs but should be > 0 for non-empty text
        let tokens = provider.estimate_tokens("hello world");
        assert!(tokens > 0);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p telos_agent -- provider::routed
```
Expected: all 7 tests pass.

- [ ] **Step 3: Commit**

```bash
git add core/src/provider/routed.rs
git commit -m "feat: add RoutedModelConfig and RoutedProvider for hint-based model routing

RoutedModelConfig maps ModelHints to concrete model names with a default
fallback. RoutedProvider implements ModelProvider by pre-creating one
DeepSeekProvider per model and delegating based on request.model_hint.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Provider Module Exports + Public API

**Files:**
- Modify: `core/src/provider/mod.rs:1-66`
- Modify: `core/src/lib.rs:82-85`

**Interfaces:**
- Produces: Public exports `ModelHint`, `RoutedModelConfig`, `RoutedProvider`

- [ ] **Step 1: Add routed module and re-exports in provider/mod.rs**

In `core/src/provider/mod.rs`, after the existing `mod openai_compat;` line:

```rust
mod openai_compat;
mod routed; // <-- add this line

#[cfg(test)]
mod test;

pub mod deepseek;
mod traits;
mod types;

pub use deepseek::{DeepSeekConfig, DeepSeekProvider};
pub use routed::{RoutedModelConfig, RoutedProvider}; // <-- add this line
pub use traits::{ErasedProvider, ModelProvider};
pub use types::{CompletionRequest, CompletionResponse, ModelHint, ProviderEvent, StopReason, TokenUsage}; // <-- add ModelHint to this line
```

- [ ] **Step 2: Add new types to crate-level re-exports in core/src/lib.rs**

In `core/src/lib.rs`, update the provider re-export block:

```rust
pub use provider::{
    CompletionRequest, CompletionResponse, DeepSeekConfig, DeepSeekProvider, ErasedProvider,
    ModelHint, ModelProvider, ProviderEvent, RoutedModelConfig, RoutedProvider, StopReason,
    TokenUsage,
};
```

- [ ] **Step 3: Verify compilation**

```bash
cargo build -p telos_agent
```
Expected: compiles clean.

- [ ] **Step 4: Commit**

```bash
git add core/src/provider/mod.rs core/src/lib.rs
git commit -m "feat: export ModelHint, RoutedModelConfig, RoutedProvider from provider module

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: Session Turn Loop Routing

**Files:**
- Modify: `core/src/runtime/session.rs:250-370` (call_provider), `core/src/runtime/session.rs:488-649` (run_turn_stream)

**Interfaces:**
- Consumes: `ModelHint` (from Task 1), `TaskPath` (existing, on `AgentConfig`)
- Produces: `AgentSession::resolve_hint()` private method
- Modifies: `call_provider()` takes `hint: ModelHint` parameter
- Modifies: `run_turn_stream()` determines hint per iteration and passes it

- [ ] **Step 1: Add resolve_hint private method to AgentSession**

In `core/src/runtime/session.rs`, add to `impl AgentSession` block (before `call_provider`):

```rust
    /// Determine the appropriate [`ModelHint`] for the current iteration.
    fn resolve_hint(
        config: &crate::config::AgentConfig,
        iteration: usize,
        previous_tool_error: bool,
        consecutive_noop: usize,
    ) -> ModelHint {
        // Fast path: everything uses execution model
        if config.path == crate::config::TaskPath::Fast {
            return ModelHint::Execution;
        }

        // Error recovery: tool failure needs re-evaluation
        if previous_tool_error {
            return ModelHint::Recovery;
        }

        // Stuck detection: repeated tool rounds with no progress
        if consecutive_noop >= 3 {
            return ModelHint::Thinking;
        }

        // First call in a turn: understand user intent, plan
        if iteration == 1 {
            return ModelHint::Thinking;
        }

        // Heavy path: periodic re-thinking every 4 tool rounds
        if config.path == crate::config::TaskPath::Heavy && iteration % 4 == 0 {
            return ModelHint::Thinking;
        }

        // Default: processing tool results is execution work
        ModelHint::Execution
    }
```

- [ ] **Step 2: Update call_provider signature and request construction**

Change the method signature in `session.rs` — add `hint: ModelHint` parameter:

```rust
    async fn call_provider<P: ModelProvider>(
        &mut self,
        provider: &P,
        tool_definitions: &[crate::tool::ToolDefinition],
        hint: ModelHint,  // <-- new parameter
    ) -> Result<(Message, StopReason, Option<TokenUsage>, Vec<TurnEvent>), AgentError> {
```

In the same method, update the `CompletionRequest` construction (~line 277) to include the hint:

```rust
            let request = CompletionRequest {
                system_prompt,
                system_prompt_blocks,
                messages: self.messages.clone(),
                tools: tool_definitions.to_vec(),
                model_hint: Some(hint),  // <-- add this line
            };
```

- [ ] **Step 3: Update run_turn_stream to track state and compute hint**

In `run_turn_stream`, add state-tracking variables after `let mut iterations = 0;` (~line 537):

```rust
            let mut iterations = 0;
            // Track state for model routing decisions
            let mut previous_tool_error = false;
            let mut consecutive_noop = 0usize;
            let mut last_assistant_text_len = 0usize;
```

Before `call_provider`, compute the hint (~line 587, before the call):

```rust
                let hint = Self::resolve_hint(
                    &self.config,
                    iterations,
                    previous_tool_error,
                    consecutive_noop,
                );
```

Update the `call_provider` invocation to pass hint:

```rust
                let (assistant_message, stop_reason, usage, provider_events) =
                    self.call_provider(provider, &tool_definitions, hint).await?;
```

After processing tool results, update the routing state. After the tool execution block (~line 640, after tool_calls handling), add:

```rust
                // Update routing state for next iteration
                previous_tool_error = tool_results.iter().any(|r| r.is_error);
                let current_text_len = assistant_message.text_content().len();
                if current_text_len <= last_assistant_text_len && !tool_calls.is_empty() {
                    consecutive_noop += 1;
                } else {
                    consecutive_noop = 0;
                }
                last_assistant_text_len = current_text_len;
```

Wait — `tool_results` isn't directly available here. The tool execution happens inside `execute_tool_calls_phase` which returns `(Message, Vec<TurnEvent>)`. The Message contains tool results. Let me adjust:

After `execute_tool_calls_phase` returns (~line 640-641):

```rust
                let (tool_message, tool_events) =
                    self.execute_tool_calls_phase(&tools, tool_calls, turn_id).await?;

                // Update routing state from tool results
                previous_tool_error = tool_message.tool_results_iter().any(|r| r.is_error);
```

And the consecutive_noop tracking can be done after the assistant message is available, before tool calls are checked:

After `self.messages.push(assistant_message.clone());` (~line 603), track noop:

```rust
                // Track whether this iteration made progress (for routing)
                let current_text_len = assistant_message.text_content().len();
                if current_text_len <= last_assistant_text_len && !tool_calls.is_empty() {
                    consecutive_noop += 1;
                } else {
                    consecutive_noop = 0;
                }
                last_assistant_text_len = current_text_len;
```

Actually, looking more carefully at the existing code structure, the `tool_calls` check needs to happen after `assistant_message` is pushed. Let me look at the exact flow:

1. `call_provider` → `assistant_message`
2. `self.messages.push(assistant_message.clone())`
3. hook phases
4. `let tool_calls = assistant_message.tool_calls()...`
5. if empty → finish turn
6. `execute_tool_calls_phase` → `tool_message`

So the state update for `previous_tool_error` should happen in step 6, and `consecutive_noop` should be tracked around step 4. Let me adjust:

```rust
                self.messages.push(assistant_message.clone());
                yield TurnEvent::Assistant(assistant_message.clone());

                // ── Update routing: track progress ─────────────────
                let current_text_len = assistant_message.text_content().len();
                if current_text_len <= last_assistant_text_len && !assistant_message.tool_calls().next().is_some() {
                    // assistant produced text but no tool calls — this is final, no noop tracking needed
                }
                last_assistant_text_len = current_text_len;
```

Hmm, this is getting complex. Let me simplify the state tracking. The key signals are:
1. `previous_tool_error` — easy, check tool results after execution
2. `consecutive_noop` — count rounds where there are tool calls but no text progress (model just calling tools in a loop)

Let me simplify:

```rust
            let mut iterations = 0;
            let mut previous_tool_error = false;
```

Then after `execute_tool_calls_phase`:
```rust
                previous_tool_error = tool_message.tool_results_iter().any(|r| r.is_error);
```

And for `consecutive_noop`, I'll skip it for the initial implementation — it's an optimization. The core signals (first iteration, errors, task path) cover the main cases.

Actually, let me re-read the design spec. It says:
- "连续 N 轮工具调用但无有效产出 → Thinking"

Let me keep it simple for now and just track it with a counter:

After `let tool_calls = assistant_message.tool_calls()...`:
```rust
                let tool_calls = assistant_message.tool_calls().cloned().collect::<Vec<_>>();
                
                // Track no-progress loops: tool calls but no text output
                if !tool_calls.is_empty() && assistant_message.text_content().is_empty() {
                    consecutive_noop += 1;
                } else if !tool_calls.is_empty() {
                    consecutive_noop = 0;
                }
```

And `previous_tool_error` is set after tool execution. Let me finalize this.

- [ ] **Step 4: Write test for resolve_hint logic**

In `core/src/runtime/session.rs`, add a test in the existing `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn resolve_hint_first_iteration_is_thinking() {
        let config = AgentConfig::default(); // TaskPath::Standard
        let hint = AgentSession::resolve_hint(&config, 1, false, 0);
        assert_eq!(hint, ModelHint::Thinking);
    }

    #[test]
    fn resolve_hint_tool_error_is_recovery() {
        let config = AgentConfig::default();
        let hint = AgentSession::resolve_hint(&config, 2, true, 0);
        assert_eq!(hint, ModelHint::Recovery);
    }

    #[test]
    fn resolve_hint_execution_default() {
        let config = AgentConfig::default();
        let hint = AgentSession::resolve_hint(&config, 2, false, 0);
        assert_eq!(hint, ModelHint::Execution);
    }

    #[test]
    fn resolve_hint_fast_path_always_execution() {
        let config = AgentConfig::default().with_path(TaskPath::Fast);
        assert_eq!(AgentSession::resolve_hint(&config, 1, false, 0), ModelHint::Execution);
        assert_eq!(AgentSession::resolve_hint(&config, 2, true, 0), ModelHint::Execution);
        assert_eq!(AgentSession::resolve_hint(&config, 5, false, 3), ModelHint::Execution);
    }

    #[test]
    fn resolve_hint_stuck_detection() {
        let config = AgentConfig::default();
        let hint = AgentSession::resolve_hint(&config, 5, false, 3);
        assert_eq!(hint, ModelHint::Thinking);
    }

    #[test]
    fn resolve_hint_heavy_periodic_rethink() {
        let config = AgentConfig::default().with_path(TaskPath::Heavy);
        assert_eq!(AgentSession::resolve_hint(&config, 1, false, 0), ModelHint::Thinking);
        assert_eq!(AgentSession::resolve_hint(&config, 2, false, 0), ModelHint::Execution);
        assert_eq!(AgentSession::resolve_hint(&config, 4, false, 0), ModelHint::Thinking);
    }
```

Note: `resolve_hint` is a private associated function (no `&self`), so it's testable without constructing an `AgentSession`.

- [ ] **Step 5: Run tests**

```bash
cargo test -p telos_agent -- runtime::session::tests
```
Expected: existing + new tests pass.

- [ ] **Step 6: Commit**

```bash
git add core/src/runtime/session.rs
git commit -m "feat: add model hint routing to turn loop

Adds resolve_hint() and state tracking for turn-phase-based model
routing. First iteration uses Thinking, tool errors trigger Recovery,
subsequent tool processing uses Execution. TaskPath::Fast bypasses
routing entirely.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: Compaction Summarization Hint

**Files:**
- Modify: `core/src/compaction/strategy.rs:99-106`

**Interfaces:**
- Consumes: `ModelHint` (from Task 1)

- [ ] **Step 1: Set Summarization hint in compaction request**

In `core/src/compaction/strategy.rs`, update the `CompletionRequest` construction (line 99):

```rust
        let summary_request = CompletionRequest {
            system_prompt: Some(
                "Summarize the following conversation history concisely, preserving key facts, decisions, and context.".into(),
            ),
            system_prompt_blocks: None,
            messages: to_summarize,
            tools: vec![],
            model_hint: Some(ModelHint::Summarization),
        };
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p telos_agent -- compaction
```
Expected: existing compaction tests pass.

- [ ] **Step 3: Commit**

```bash
git add core/src/compaction/strategy.rs
git commit -m "feat: set Summarization hint on compaction requests

Ensures conversation summarization uses the fast model when routing
is enabled.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 6: CLI Integration — Flags, Config Parsing, Provider Wiring

**Files:**
- Modify: `cli/src/cli.rs:40-77` (SharedOptions)
- Modify: `cli/src/config.rs:1-454` (FileConfig, build_provider, ResolvedProvider)

**Interfaces:**
- Consumes: `RoutedModelConfig`, `RoutedProvider`, `ModelHint` (from Tasks 1-3)
- Produces: `--thinking-model`, `--fast-model` CLI flags
- Produces: `ResolvedProvider::Routed(RoutedProvider)` variant
- Produces: `[agent.models]` TOML config parsing

- [ ] **Step 1: Add CLI flags to SharedOptions**

In `cli/src/cli.rs`, add to `SharedOptions` struct (after the existing `model` field):

```rust
    /// Model name for the thinking/reasoning model (planning, complex decisions).
    #[clap(long, env = "TELOS_THINKING_MODEL")]
    pub thinking_model: Option<String>,

    /// Model name for the fast/execution model (tool calls, file ops, simple tasks).
    #[clap(long, env = "TELOS_FAST_MODEL")]
    pub fast_model: Option<String>,
```

- [ ] **Step 2: Add models section to FileConfig**

In `cli/src/config.rs`, add a `ModelsSection` struct:

```rust
/// Model routing configuration from [agent.models] TOML section.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelsSection {
    pub thinking: Option<String>,
    pub fast: Option<String>,
}
```

Add it to `AgentSection`:

```rust
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentSection {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub max_iterations: Option<usize>,
    pub models: Option<ModelsSection>,  // <-- add this line
}
```

- [ ] **Step 3: Add Routed variant to ResolvedProvider**

```rust
pub enum ResolvedProvider {
    DeepSeek(DeepSeekProvider),
    Routed(RoutedProvider),   // <-- add this variant
    Mock(MockProvider),
}
```

- [ ] **Step 4: Update build_provider to create RoutedProvider when dual models configured**

In `cli/src/config.rs`, in the `ProviderArg::Deepseek` match arm of `build_provider`:

```rust
        ProviderArg::Deepseek => {
            let default_model = options
                .model
                .clone()
                .or_else(|| config.agent.as_ref()?.model.clone())
                .unwrap_or_else(|| "deepseek-v4-flash".into());

            let thinking_model = options
                .thinking_model
                .clone()
                .or_else(|| config.agent.as_ref()?.models.as_ref()?.thinking.clone());

            let fast_model = options
                .fast_model
                .clone()
                .or_else(|| config.agent.as_ref()?.models.as_ref()?.fast.clone());

            let api_key =
                resolve_api_key(provider, options.api_key.clone(), config_env, "DEEPSEEK_API_KEY")?;

            match (thinking_model, fast_model) {
                (Some(thinking), Some(fast)) if thinking != fast => {
                    let routed_config = RoutedModelConfig::dual(api_key, thinking, fast);
                    Ok(ResolvedProvider::Routed(RoutedProvider::new(routed_config)))
                }
                _ => {
                    // Single model or both same — use plain DeepSeekProvider
                    let model = thinking_model
                        .or(fast_model)
                        .unwrap_or(default_model);
                    let cfg = DeepSeekConfig::new(api_key, model);
                    Ok(ResolvedProvider::DeepSeek(DeepSeekProvider::new(cfg)))
                }
            }
        }
```

- [ ] **Step 5: Update build_provider_from_onboarding for consistency**

```rust
pub fn build_provider_from_onboarding(result: &OnboardingResult) -> Result<ResolvedProvider> {
    match result.provider {
        ProviderArg::Deepseek => {
            let cfg = DeepSeekConfig::new(&result.api_key, &result.model);
            Ok(ResolvedProvider::DeepSeek(DeepSeekProvider::new(cfg)))
        }
        ProviderArg::Mock => Ok(ResolvedProvider::Mock(MockProvider::new(vec![]))),
    }
}
```

- [ ] **Step 6: Update runner.rs to match ResolvedProvider::Routed**

In `cli/src/runner.rs:60-73` (in `run_single`):

```rust
    match provider {
        ResolvedProvider::DeepSeek(p) => {
            run_with_provider(&mut session, &p, &tools, prompt, memory_store.clone()).await?;
        }
        ResolvedProvider::Routed(p) => {
            run_with_provider(&mut session, &p, &tools, prompt, memory_store.clone()).await?;
        }
        ResolvedProvider::Mock(_) => {
            // ... existing mock handling ...
        }
    }
```

- [ ] **Step 7: Update lib.rs build_erased_provider and build_erased_from_onboarding**

In `cli/src/lib.rs:221-235`:

```rust
pub(crate) fn build_erased_provider(
    options: &cli::SharedOptions,
    config: &config::FileConfig,
) -> Result<Arc<dyn telos_agent::ModelProvider>> {
    match config::build_provider(options, config)? {
        config::ResolvedProvider::DeepSeek(p) => Ok(Arc::new(p)),
        config::ResolvedProvider::Routed(p) => Ok(Arc::new(p)),
        config::ResolvedProvider::Mock(p) => Ok(Arc::new(p)),
    }
}

pub(crate) fn build_erased_from_onboarding(
    onb: &onboarding::OnboardingResult,
) -> Result<Arc<dyn telos_agent::ModelProvider>> {
    match config::build_provider_from_onboarding(onb)? {
        config::ResolvedProvider::DeepSeek(p) => Ok(Arc::new(p)),
        config::ResolvedProvider::Routed(p) => Ok(Arc::new(p)),
        config::ResolvedProvider::Mock(p) => Ok(Arc::new(p)),
    }
}
```

- [ ] **Step 8: Build and fix any compilation errors**

```bash
cargo build
```
Expected: compiles clean.

- [ ] **Step 9: Run full test suite**

```bash
cargo test --workspace
```
Expected: all tests pass.

- [ ] **Step 10: Add integration test for dual model config**

In `cli/src/config.rs` test module, add:

```rust
    #[test]
    fn build_provider_with_dual_models_creates_routed() {
        let options = SharedOptions {
            api_key: Some("sk-test".into()),
            thinking_model: Some("deepseek-v4-pro".into()),
            fast_model: Some("deepseek-v4-flash".into()),
            ..Default::default()
        };
        let config = FileConfig {
            agent: Some(AgentSection {
                provider: Some("deepseek".into()),
                ..Default::default()
            }),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        assert!(matches!(result, ResolvedProvider::Routed(_)));
    }

    #[test]
    fn build_provider_with_same_models_creates_plain_deepseek() {
        let options = SharedOptions {
            api_key: Some("sk-test".into()),
            thinking_model: Some("deepseek-v4-pro".into()),
            fast_model: Some("deepseek-v4-pro".into()),
            ..Default::default()
        };
        let config = FileConfig {
            agent: Some(AgentSection {
                provider: Some("deepseek".into()),
                ..Default::default()
            }),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        assert!(matches!(result, ResolvedProvider::DeepSeek(_)));
    }

    #[test]
    fn build_provider_without_model_flags_creates_plain_deepseek() {
        let options = SharedOptions {
            api_key: Some("sk-test".into()),
            model: Some("deepseek-v4-flash".into()),
            ..Default::default()
        };
        let config = FileConfig {
            agent: Some(AgentSection {
                provider: Some("deepseek".into()),
                ..Default::default()
            }),
            ..FileConfig::default()
        };
        let result = build_provider(&options, &config).unwrap();
        assert!(matches!(result, ResolvedProvider::DeepSeek(_)));
    }
```

- [ ] **Step 11: Run tests**

```bash
cargo test -p telos-cli
```
Expected: all tests pass including new integration tests.

- [ ] **Step 12: Final workspace test**

```bash
cargo test --workspace
```
Expected: all tests pass.

- [ ] **Step 13: Commit**

```bash
git add cli/src/cli.rs cli/src/config.rs cli/src/runner.rs cli/src/lib.rs
git commit -m "feat: add CLI flags and config for dual model routing

Adds --thinking-model and --fast-model CLI flags with TOML
[agent.models] config section. ResolvedProvider gains Routed variant.
When thinking and fast model differ, creates RoutedProvider; otherwise
falls back to plain DeepSeekProvider for backward compatibility.

Co-Authored-By: Claude <noreply@anthropic.com>"
```
