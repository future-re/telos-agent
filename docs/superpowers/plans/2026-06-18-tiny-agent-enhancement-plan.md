# tiny-agent-core Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Skills, Prompt, Memory/Profiles, MCP, Fork, Hooks, and Task systems into tiny-agent-core.

**Architecture:** Phase 1 builds the intelligence layer (Skills → Prompt → Memory → Profiles). Phase 2 adds external connectivity (MCP + Web tools). Phase 3 adds parallel execution (Fork + enhanced Hooks + Task tracking).

**Tech Stack:** Rust 2024 edition, tokio, serde_json, async-trait, pulseengine/mcp (Phase 2), reqwest (Phase 2).

## Global Constraints

- Rust edition: 2024
- Rust minimum version: 1.96
- Async runtime: tokio
- No new crate dependencies in Phase 1 or Phase 3
- All new modules are `pub` in `lib.rs`
- Tests use existing pattern in `tests/integration_tests.rs`

---

### Task 1: Skill type and file format

**Files:**
- Create: `src/skills/mod.rs`
- Create: `src/skills/loader.rs`
- Modify: `src/lib.rs` — add `pub mod skills;` and re-exports
- Modify: `Cargo.toml` — add `serde_yaml = "0.9"`

**Interfaces:**
- Produces: `Skill { name, description, when_to_use, prompt, arguments, body, source }`, `SkillArg { name, description, required }`, `SkillSource { Bundled, User, Project, Managed }`, `SkillLoader::load_from_dir(path) -> Result<Vec<Skill>, std::io::Error>`, `SkillLoader::load_bundled_skills() -> Vec<Skill>`
- Consumes: nothing

- [ ] **Step 1: Write `src/skills/mod.rs` with types**

```rust
//! Skills system — user-defined slash-commands loaded from markdown files.

use std::path::Path;

/// A loaded skill ready for invocation.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub prompt: String,
    pub arguments: Vec<SkillArg>,
    pub body: String,
    pub source: SkillSource,
}

#[derive(Debug, Clone)]
pub struct SkillArg {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    Bundled,
    Managed,
    Project,
    User,
}
```

- [ ] **Step 2: Write `src/skills/loader.rs` with YAML frontmatter parser**

```rust
use std::path::Path;
use crate::skills::{Skill, SkillArg, SkillSource};

pub struct SkillLoader;

impl SkillLoader {
    pub fn load_from_dir(dir: &Path) -> Result<Vec<Skill>, std::io::Error> {
        let mut skills = Vec::new();
        if !dir.exists() { return Ok(skills); }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(true, |ext| ext != "md") { continue; }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Some(skill) = Self::parse_skill(&content, SkillSource::Project) {
                    skills.push(skill);
                } else {
                    tracing::warn!("failed to parse skill file: {}", path.display());
                }
            }
        }
        Ok(skills)
    }

    fn parse_skill(content: &str, source: SkillSource) -> Option<Skill> {
        let content = content.trim();
        let rest = content.strip_prefix("---")?;
        let (frontmatter, body) = rest.split_once("\n---")?;
        let body = body.trim().to_string();
        let fm: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
        let name = fm.get("name")?.as_str()?.to_string();
        let description = fm.get("description")?.as_str()?.to_string();
        let when_to_use = fm.get("whenToUse").and_then(|v| v.as_str()).map(String::from);
        let prompt = fm.get("prompt")?.as_str()?.to_string();
        let arguments = fm.get("arguments").and_then(|v| v.as_sequence()).map(|args| {
            args.iter().map(|a| SkillArg {
                name: a.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                description: a.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                required: a.get("required").and_then(|v| v.as_bool()).unwrap_or(false),
            }).collect()
        }).unwrap_or_default();
        Some(Skill { name, description, when_to_use, prompt, arguments, body, source })
    }
}
```

- [ ] **Step 3: Add to lib.rs** — `pub mod skills;` and re-exports
- [ ] **Step 4: Add serde_yaml** to Cargo.toml dependencies
- [ ] **Step 5: Integration test** — `skill_loader_parses_valid_markdown` in `tests/integration_tests.rs`
- [ ] **Step 6: Run tests, commit**

---

### Task 2: SkillRegistry with override priority

**Files:**
- Create: `src/skills/registry.rs`
- Modify: `src/skills/mod.rs` — add `pub mod registry; pub use registry::SkillRegistry;`
- Modify: `src/lib.rs` — add `pub use skills::SkillRegistry;`

**Interfaces:**
- Produces: `SkillRegistry { skills: HashMap<String, Skill> }`, `SkillRegistry::new()`, `register(skill: Skill)`, `get(name: &str) -> Option<&Skill>`, `inject_skills_from_dir(dir: &Path, source: SkillSource) -> io::Result<()>`, `list() -> Vec<&Skill>`, `render_for_prompt() -> String`
- Consumes: `Skill`, `SkillSource`, `SkillLoader` from Task 1

- [ ] **Step 1: Write test** — `skill_registry_override_priority` and `skill_registry_render_for_prompt` in integration tests
- [ ] **Step 2: Write `src/skills/registry.rs`** — SkillRegistry with HashMap storage, last-write-wins override priority, render_for_prompt()
- [ ] **Step 3: Update mod.rs and lib.rs** — re-exports
- [ ] **Step 4: Run tests, commit**

---

### Task 3: SkillTool — model-invokable skill execution

**Files:**
- Create: `src/tools/skill.rs`
- Modify: `src/tools/mod.rs` — add module and re-export
- Modify: `src/lib.rs` — add re-export

**Interfaces:**
- Produces: `SkillTool::new(registry: Arc<SkillRegistry>)`, implements `Tool` trait
- Consumes: `SkillRegistry` from Task 2, `Tool` trait, `ToolContext`, `ToolOutput`

- [ ] **Step 1: Write test** — `skill_tool_invokes_and_returns_prompt` in integration tests
- [ ] **Step 2: Write `src/tools/skill.rs`** — SkillTool with definition(), check_permission() (always Allow), invoke() (substitutes {{args}})
- [ ] **Step 3: Update src/tools/mod.rs** — add `mod skill; pub use skill::SkillTool;`
- [ ] **Step 4: Run tests, commit**

---

### Task 4: Bundled skills

**Files:**
- Create: `src/skills/bundled/verify.md`, `debug.md`, `remember.md`, `brainstorm.md`, `update-config.md`
- Modify: `src/skills/loader.rs` — add `load_bundled_skills()` using `include_str!`

**Interfaces:**
- Produces: `SkillLoader::load_bundled_skills() -> Vec<Skill>`
- Consumes: Nothing new

- [ ] **Step 1: Create 5 bundled skill .md files** under `src/skills/bundled/`
- [ ] **Step 2: Add `load_bundled_skills()`** to loader.rs using `include_str!`
- [ ] **Step 3: Test** — `bundled_skills_load_successfully` verifies >=5 skills with non-empty name/description/prompt
- [ ] **Step 4: Run tests, commit**

---

### Task 5: PromptSection trait and PromptAssembly

**Files:**
- Create: `src/prompt/mod.rs`
- Create: `src/prompt/section.rs`
- Create: `src/prompt/assembly.rs`
- Modify: `src/lib.rs` — add `pub mod prompt;`

**Interfaces:**
- Produces: `PromptStability { Static, Dynamic }`, `PromptSection` trait (`name()`, `stability()`, `render()`), `PromptAssembly::new()`, `add_static()`, `add_dynamic()`, `build() -> String`
- Consumes: nothing

- [ ] **Step 1: Write test** — verify static sections cached, dynamic sections re-rendered
- [ ] **Step 2: Write `section.rs`** — PromptStability enum + PromptSection trait
- [ ] **Step 3: Write `assembly.rs`** — PromptAssembly with tokio::sync::Mutex static cache
- [ ] **Step 4: Write `mod.rs`** — module exports
- [ ] **Step 5: Update lib.rs**, run tests, commit

---

### Task 6: Built-in prompt sections

**Files:**
- Create: `src/prompt/builtins.rs`
- Modify: `src/prompt/mod.rs` — add `pub mod builtins;`

**Interfaces:**
- Produces: `IdentitySection`, `ToolsSection`, `DateSection`, `CwdSection`, `GitStatusSection`, `SkillsSection`
- Consumes: `PromptSection` trait, `ToolRegistry`, `SkillRegistry`

- [ ] **Step 1: Write `builtins.rs`** — all 6 sections with their render logic
- [ ] **Step 2: Integration test** — `builtin_prompt_sections_render_without_error`
- [ ] **Step 3: Run tests, commit**

---

### Task 7: Integrate PromptAssembly into AgentConfig and runtime

**Files:**
- Modify: `src/config.rs` — replace `system_prompt: Option<String>` with `base_system_prompt: Option<String>` + `prompt_assembly: Option<Arc<PromptAssembly>>`
- Modify: `src/runtime.rs` — use PromptAssembly in AgentSession::new and turn loop

- [ ] **Step 1: Modify AgentConfig** — replace system_prompt field, update Default and Debug impls
- [ ] **Step 2: Modify runtime.rs** — AgentSession::new and turn loop use PromptAssembly
- [ ] **Step 3: Fix all tests** referencing system_prompt → base_system_prompt
- [ ] **Step 4: Integration test** — prompt_assembly_integration_with_session
- [ ] **Step 5: Run all tests, commit**
