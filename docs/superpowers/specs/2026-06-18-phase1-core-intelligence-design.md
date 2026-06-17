# Phase 1: Core Intelligence Layer — Design Spec

**Date:** 2026-06-18
**Status:** Design approved
**Scope:** Skills System + Prompt System Refactor + Memory System + Context Profiles

---

## Architecture Overview

```
┌──────────────┐    ┌──────────────────┐    ┌──────────────┐
│   Skills     │───▶│  Prompt System   │◀───│   Memory     │
│   System     │    │    (refactored)   │    │   System     │
└──────────────┘    └────────┬─────────┘    └──────┬───────┘
                             │                     │
                             ▼                     ▼
                      AgentSession gets:    ┌──────────────┐
                      - Skill awareness    │   Profiles   │
                      - Dynamic context    │ (distillation)│
                      - Memory injection   └──────────────┘
```

**Module dependencies:**
- Skills → Prompt: skills list injected into system prompt
- Memory → Prompt: relevant memories + profiles injected
- Memory → Consolidation: periodic distillation of raw memories into profiles/workflows
- Profiles → Prompt: always-on context portrait of user, project, and active state

---

## 1. Skills System

### 1.1 File Format

Skills are Markdown files with YAML frontmatter:

```markdown
---
name: my-skill
description: What this skill does
whenToUse: When to suggest this skill
prompt: |
  You are now in 'my-skill' mode.
  Context: {{args}}
arguments:
  - name: args
    description: Optional arguments
    required: false
---
Body text — also passed to the model, appended after the prompt.
```

### 1.2 Directory Layout

```
~/.tiny-agent/skills/          # user scope
  my-custom-workflow.md

.tiny-agent/skills/             # project scope
  code-review.md

src/skills/bundled/             # compiled-in (built-in)
  brainstorm.md
  debug.md
  verify.md
  remember.md
  update-config.md
```

**Loading priority (later overrides earlier):** bundled → managed → project → user

### 1.3 Core Types

```rust
struct Skill {
    name: String,
    description: String,
    when_to_use: Option<String>,
    prompt: String,                 // injected into conversation
    arguments: Vec<SkillArg>,       // {{arg_name}} template substitution
    body: String,                   // markdown after frontmatter
    source: SkillSource,
}

struct SkillArg {
    name: String,
    description: String,
    required: bool,
}

enum SkillSource { Bundled, User, Project, Managed }
```

### 1.4 Module Structure

```
src/skills/
  mod.rs          — module exports
  loader.rs       — directory scanning + YAML frontmatter parsing
  registry.rs     — SkillRegistry (HashMap + queries)
  bundled/        — 5-6 built-in skills (.md compiled into binary)
```

### 1.5 Invocation Flow

```
User types "/verify"
  → CLI parses skill name
  → SkillRegistry.get("verify")
  → skill.prompt (with {{args}} substituted) injected into current turn's system message
  → model executes according to skill instructions

Model calls Skill tool:
  → agent invokes Skill { name: "verify", args: "..." }
  → SkillTool.invoke() → returns parsed prompt
  → model receives prompt content and executes accordingly
```

### 1.6 Key Design Decisions

- **Skill is NOT a shell script** — it's a prompt injected to the model; the model executes
- **SkillTool is always allowed** — no permission check (user-defined, no security risk)
- **Prompt template uses `{{var}}`** — simple string replace, no handlebars/tera dependency
- **No hot-reload in Phase 1** — loaded once at AgentSession creation
- **No argument validation beyond required/optional** — model self-corrects

---

## 2. Prompt System Refactor

### 2.1 Architecture: Modular Sections with Cache Awareness

```
┌─────────────────────────────────────────────┐
│              System Prompt                    │
├─────────────────────────────────────────────┤
│  Static (unchanging, cacheable)               │
│  ┌─ Identity: "You are tiny-agent, a CLI..." │
│  ├─ Tool definitions: all Tool JSON Schemas   │
│  ├─ Agent definitions: available subagent types│
│  └─ Core instructions: basic behavior rules   │
├─────────────────────────────────────────────┤
│  Dynamic (per-turn, NOT cached)               │
│  ┌─ Date/time: current date                   │
│  ├─ Git status: current branch, changed files │
│  ├─ Working directory                         │
│  ├─ Profile section: user + project + active  │
│  ├─ Memory section: relevant memories injected│
│  ├─ Skills section: available skills list     │
│  └─ MCP section: available MCP tools (Phase 2)│
└─────────────────────────────────────────────┘
```

### 2.2 Core Types

```rust
trait PromptSection: Send + Sync {
    fn name(&self) -> &str;
    fn stability(&self) -> PromptStability;
    async fn render(&self, ctx: &SessionContext) -> String;
}

enum PromptStability {
    Static,     // cached for entire session
    Dynamic,    // regenerated each turn
}

struct PromptAssembly {
    static_sections: Vec<Box<dyn PromptSection>>,
    dynamic_sections: Vec<Box<dyn PromptSection>>,
}
```

### 2.3 Built-in Sections (Phase 1)

| Section | Stability | Content |
|---------|-----------|---------|
| identity | Static | "You are tiny-agent, a CLI coding assistant..." |
| tools | Static | All Tool definitions (name + description + JSON Schema) |
| agents | Static | Available subagent types |
| date | Dynamic | Current date `2026-06-18` |
| git-status | Dynamic | Current branch + changed file list |
| cwd | Dynamic | Current working directory |
| profile | Static* | User + project + active state profiles (loaded once per session) |
| memory | Dynamic | Relevant memories for current task |
| skills | Dynamic | Loaded skills list (name + description + whenToUse) |

*Profile is treated as Static for caching: loaded once at session start, not re-read mid-session.

### 2.4 Generation Flow

```rust
impl AgentSession {
    async fn build_system_prompt(&self, ctx: &SessionContext) -> String {
        let static_parts: String = self.static_sections.iter()
            .map(|s| s.render(ctx)).join("\n\n");

        let dynamic_parts: String = self.dynamic_sections.iter()
            .map(|s| s.render(ctx)).join("\n\n");

        format!("{static_parts}\n\n---\n\n{dynamic_parts}")
    }
}
```

### 2.5 AgentConfig Changes

```rust
// Old
system_prompt: Option<String>

// New — replaces old field
base_system_prompt: Option<String>,     // appended to identity section
extra_sections: Vec<Box<dyn PromptSection>>, // user/plugin custom sections
```

### 2.6 Design Decisions

- **No template engine** — trait + `format!()` is sufficient; avoids handlebars/tera dependency
- **Static sections frozen after tool registration** — tool/agent definitions don't change mid-session
- **Dynamic sections are lightweight** — file reads + string concatenation, no network I/O
- **Section order is fixed** — identity → tools → agents → profile → date → git → cwd → skills → memory → user_custom

---

## 3. Memory System

### 3.1 Memory Taxonomy

```
.tiny-agent/memory/
├── MEMORY.md                      # index (one line per memory)
├── scripts/                       # generated scripts
│   ├── deploy-staging.sh.md
│   └── clean-old-branches.sh.md
├── commands/                      # useful command snippets
│   ├── find-large-files.md
│   └── git-squash-workflow.md
├── patterns/                      # code patterns/templates
│   ├── rust-error-handling.md
│   └── dockerfile-pattern.md
├── facts/                         # facts/preferences
│   ├── user-likes-short-names.md
│   └── prod-config.md
├── workflows/                     # multi-step workflows (consolidation output)
│   ├── release-checklist.md
│   └── new-service-setup.md
├── _archived/                     # deprecated/pruned (never deleted)
└── profile/                       # context profiles
    ├── user.md
    ├── project.md
    └── active.md
```

### 3.2 Memory File Format

```markdown
---
name: deploy-staging
description: Script to deploy to staging environment
category: script              # script | command | pattern | fact | workflow
tags: [deploy, staging, docker, k8s]
created: 2026-06-18
updated: 2026-06-18
status: working               # working | needs-fix | deprecated
times_used: 3
confidence: high              # low | medium | high
related: [[docker-setup]], [[staging-config]]
source_session: a1b2c3d4
---

# Deploy to Staging

## Script
```bash
#!/bin/bash
set -euo pipefail
docker build -t app:staging .
docker push registry.example.com/app:staging
kubectl rollout restart deployment/app -n staging
kubectl rollout status deployment/app -n staging --timeout=120s
```

## When to use
After merging to main, before notifying QA.

## Known issues
- Sometimes needs `--no-cache` if base image changed
- Timeout at 120s might be too short for cold starts

## Evolution
- v1: just `docker build && docker push` (failed: no rollout)
- v2: added rollout restart (current)
```

### 3.3 Memory Lifecycle

```
Created → working → needs-fix → working → deprecated
              │         │
              └── used again (model found it doesn't work, flags it)
```

**Golden rule: never auto-delete.** Only move to `_archived/`. Mark as `deprecated`.

### 3.4 Retrieval Priority (for prompt injection)

```
1. [[related links]] → traverse the link graph
2. tags match current task
3. category match (command tasks → prefer scripts/commands)
4. recency + times_used → frequently used + fresh first
```

### 3.5 Memory Tools (model-facing)

| Action | Tool | When |
|--------|------|------|
| Write | `MemoryWrite` | Generated useful script, learned user preference |
| Read | `MemoryRead` | Checking for existing solution before acting |
| Search | `MemoryGrep` | "What was that deploy script called?" |
| Edit | `MemoryEdit` | Script broke, needs fixing |
| Status | `MemoryStatus` | Mark deprecated / needs-fix |

### 3.6 Module Structure

```
src/memory/
  mod.rs             — module exports + MemoryStore
  format.rs          — YAML frontmatter parsing + markdown body
  index.rs           — MEMORY.md read/write
  query.rs           — retrieval by category/tags/related/status
  tool.rs            — MemoryRead/Write/Grep/Edit/Status
  consolidation.rs   — ConsolidationEngine entry point
  consolidation/
    trigger.rs       — consolidation trigger conditions
    orient.rs        — orient phase: scan index, identify candidates
    gather.rs        — gather phase: read full content, group by topic
    consolidate.rs   — consolidate phase: LLM-driven distillation
    prune.rs         — prune phase: deprecate, archive, merge duplicates
  profile.rs         — ProfileManager: distill facts into profiles
```

### 3.7 Design Decisions

- **Self-implemented, NOT using merlion-memory** — our format is richer (subdirectories, status, tags, [[links]], evolution log), core logic ~500 lines
- **`serde_yaml` for frontmatter** — already in dependency tree
- **No vector search in Phase 1** — keyword + tag + link traversal is sufficient. Semantic search via vectorlite or opencode-memory can be added in Phase 2+
- **Memory tools bypass permission** — model writing to its own memory store is safe

---

## 4. Memory Consolidation ("Dream")

### 4.1 Core Concept

```
Single memories        Repeated use           Workflows
────────               ────────────           ────────────
scripts/               scripts/               workflows/
commands/    ──────►   patterns/    ──────►   + patterns/
facts/                 cross-linked             full steps

low confidence         medium confidence       high confidence
times_used: 0-1        times_used: 2-5         times_used: 5+
scattered              associating             stable + reusable
```

### 4.2 Consolidation Pipeline

```
┌─────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌────────┐
│ Orient  │──▶│  Gather  │──▶│Consolidate│──▶│  Prune   │──▶│ Index  │
│ Scan    │   │  Collect │   │  Distill  │   │  Clean   │   │ Update │
└─────────┘   └──────────┘   └──────────┘   └──────────┘   └────────┘
```

### 4.3 Trigger (Three Gates)

```rust
struct ConsolidationTrigger {
    time_gate:    Duration,   // > 24h since last consolidation
    session_gate: u32,        // > 5 sessions since last consolidation
    memory_gate:  u32,        // > 10 new/modified memories
    // ALL three must pass
}
```

### 4.4 Phase Details

**Phase 1 — Orient (pure rules, no LLM):**
- Scan `MEMORY.md` index
- Identify: newly created memories, needs-fix, deprecated-but-linked, trending tags, unused scripts
- Output: candidate list for merge/upgrade/cleanup

**Phase 2 — Gather (pure rules + lightweight LLM):**
- Read full content of candidate memories
- Group by topic using tags + [[links]] + LLM topic assignment
- Output: topic groups (e.g., "deployment": [4 related memories])

**Phase 3 — Consolidate (LLM-driven, core distillation):**
For each topic group, LLM produces:
- A workflow or pattern memory with full steps
- Links back to source memories (`derived_from: 4 memories over 3 sessions`)
- Common pitfalls section
- Evolution log

**Phase 4 — Prune (rules + user confirmation):**
- Mark deprecated memories
- Merge exact duplicates (keep higher times_used)
- Archive: 30 days unused + needs-fix → `_archived/`
- Dry-run mode: show plan, get user confirmation before execution

**Phase 5 — Index Update:**
- Rewrite `MEMORY.md`
- Update `[[link]]` references
- Update `related` fields

### 4.5 Iterative Consolidation

```
Session 1-5:    scattered scripts/commands generated
       ↓
1st Dream:      scripts → patterns (medium confidence)
       ↓
Session 6-10:   patterns used, validated, corrected
       ↓
2nd Dream:      patterns → workflows (high confidence)
       ↓
Session 11+:    workflows stable, become project knowledge base
```

### 4.6 Execution Modes

```rust
// 1. Background auto-check (post-sampling Stop hook)
impl Hook for MemoryConsolidationHook {
    fn phase() -> HookPhase { HookPhase::Stop }
    fn run(ctx) {
        if consolidation_trigger.check(&memory_store) {
            tokio::spawn(async move {
                consolidation_engine.run(&memory_store).await;
            });
        }
    }
}

// 2. Manual trigger
// "/remember consolidate" → Skill → ConsolidateMemory tool
```

### 4.7 Design Decisions

- **Dream uses lightweight model** — separate summary-level model (haiku tier), not the main provider; avoids burning context tokens
- **Dry-run mode** — always show plan before executing destructive changes
- **Git safety net** — memory directory is inside the project (or ~/.tiny-agent), git tracks changes
- **Archived, never deleted** — `_archived/` directory, recoverable
- **Phase 1: Orient + Index only (pure rules)** — LLM-driven Consolidate + Prune added in Phase 1.5

---

## 5. Context Profiles

### 5.1 Three Profiles

```
┌─────────────────────────────────────────────┐
│           CONTEXT PROFILE (~500-1000 tokens) │
│           Always injected into system prompt │
├─────────────────────────────────────────────┤
│  User Profile    │  Who am I? Preferences?  │
│  Project Profile │  What tech stack? Conventions?│
│  Active State    │  What am I doing now?    │
└─────────────────────────────────────────────┘
        ▲                    ▲
        │  distilled from    │  distilled from
        │                    │
  ┌─────┴──────┐      ┌─────┴──────┐
  │  Memory    │      │  Session   │
  │  Store     │      │  History   │
  └────────────┘      └────────────┘
```

### 5.2 Profile Files

**User Profile** (`~/.tiny-agent/profile/user.md`):
```markdown
---
type: profile
profile: user
updated: 2026-06-18
derived_from: 23 memories across 8 sessions
---

## Identity
- Full-stack Rust developer, 5 years experience
- Prefers explicitness over magic

## Preferences
- Short variable names ok: ctx, tx, rx
- Error handling: prefers thiserror over manual Display
- Testing: integration tests > unit tests

## Communication
- Wants direct answers, no fluff
- Likes trade-off analysis before decisions

## Anti-preferences
- Don't add dependencies without asking
- Don't refactor unrelated code
```

**Project Profile** (`.tiny-agent/profile/project.md`):
```markdown
---
type: profile
profile: project
updated: 2026-06-18
---

## tiny-agent-core
- Rust 2024 edition, tokio async runtime
- LLM providers via ModelProvider trait
- Security: fail-closed everywhere, tree-sitter for bash analysis

## Tech Stack
- Rust 1.96+, async-openai 0.41, tokio, serde, tree-sitter

## Conventions
- Module structure: one concept per file
- Errors: thiserror enums
- Tests: integration tests in tests/, mock in src/mock.rs
```

**Active State** (`.tiny-agent/profile/active.md`):
```markdown
---
type: profile
profile: active
updated: 2026-06-18
---

## Active Work
- Phase 1: Skills + Prompt + Memory system implementation
- Just finished: Bash security module (commit 328e848)

## Open Decisions
- Skills: custom or reuse existing crate?
- Memory: self-built vs merlion-memory?

## Blocked
- Nothing currently blocked
```

### 5.3 Profile Position in System Prompt

```
System Prompt
├── identity: "You are tiny-agent..."
├── tools: [...definitions...]
├── agents: [...]
├── ── PROFILE BOUNDARY ──
├── user profile    ← always injected
├── project profile ← always injected
├── active state    ← always injected
├── ── MEMORY BOUNDARY ──
├── relevant memories ← on-demand, task-specific
├── date / git / cwd
```

### 5.4 Update Cadence

| Profile | Update Trigger | Frequency | Method |
|---------|---------------|-----------|--------|
| active.md | Every session end | High | Lightweight: extracted from memory index + session metrics, no LLM needed |
| user.md | Dream consolidation | Low | Heavy: LLM scans all facts/ with type=user, distills preferences |
| project.md | Dream consolidation | Low | Heavy: LLM scans facts/ + patterns/ + workflows/, distills conventions |

### 5.5 Module Integration

```rust
struct ProfileManager {
    user_profile:    ProfileSlot,   // low-frequency update
    project_profile: ProfileSlot,   // low-frequency update
    active_state:    ActiveState,   // high-frequency lightweight update
}

// Profile is a PromptSection — rendered once at session start
impl PromptSection for ProfileSection {
    fn stability(&self) -> PromptStability { PromptStability::Static }
    fn name(&self) -> &str { "profile" }
    async fn render(&self, ctx: &SessionContext) -> String { ... }
}
```

### 5.6 Design Decisions

- **Profile vs Memory distinction**: profiles answer "who/what/where" (always on), memories answer "how exactly" (on-demand)
- **Profiles are ~500-1000 tokens fixed budget** — forces distillation, not raw dump
- **Active state updated without LLM** — simple extraction from memory index and git state
- **User profile stored in ~/.tiny-agent/** — follows the user across projects
- **Project profile stored in .tiny-agent/** — per-project, committed to git

---

## 6. File Layout Summary

After Phase 1, the project structure becomes:

```
tiny-agent-core/
├── src/
│   ├── skills/                    # NEW
│   │   ├── mod.rs
│   │   ├── loader.rs
│   │   ├── registry.rs
│   │   └── bundled/
│   │       ├── brainstorm.md
│   │       ├── debug.md
│   │       ├── verify.md
│   │       ├── remember.md
│   │       └── update-config.md
│   ├── prompt/                    # NEW (extracted from config.rs)
│   │   ├── mod.rs
│   │   ├── section.rs             # PromptSection trait
│   │   ├── assembly.rs            # PromptAssembly
│   │   └── sections/
│   │       ├── identity.rs
│   │       ├── tools.rs
│   │       ├── agents.rs
│   │       ├── date.rs
│   │       ├── git.rs
│   │       ├── cwd.rs
│   │       ├── profile.rs
│   │       ├── memory.rs
│   │       └── skills.rs
│   ├── memory/                    # NEW
│   │   ├── mod.rs
│   │   ├── format.rs
│   │   ├── index.rs
│   │   ├── query.rs
│   │   ├── tool.rs
│   │   ├── profile.rs
│   │   ├── consolidation.rs
│   │   └── consolidation/
│   │       ├── trigger.rs
│   │       ├── orient.rs
│   │       ├── gather.rs
│   │       ├── consolidate.rs
│   │       └── prune.rs
│   ├── config.rs                  # MODIFIED: system_prompt → base_system_prompt + extra_sections
│   ├── runtime.rs                 # MODIFIED: uses PromptAssembly
│   ├── hooks.rs                   # MODIFIED: add MemoryConsolidationHook
│   ├── tool/mod.rs                # MODIFIED: register Skills/Memory tools
│   └── ... (existing files unchanged)
├── Cargo.toml                     # MODIFIED: add serde_yaml (if not already present)
└── docs/superpowers/specs/        # NEW
    └── 2026-06-18-phase1-core-intelligence-design.md
```

---

## 7. Dependencies Added

- `serde_yaml` — YAML frontmatter parsing for skills and memory (may already exist transitively)

No other new dependencies in Phase 1. Template rendering uses simple `{{var}}` string replacement. No handlebars, no tera, no vector database.

---

## 8. Testing Strategy

### Skills
- Parse all bundled skills at startup (compile-time test)
- Test YAML frontmatter parsing with valid and invalid inputs
- Test argument template substitution
- Test loading priority (user overrides project overrides bundled)

### Prompt
- Verify all sections render without panicking
- Verify static sections don't change between turns
- Verify dynamic sections reflect current state (date changes, git status reflects checkout)
- Verify profile injection appears correctly in final prompt

### Memory
- Test CRUD operations via MemoryRead/Write/Edit tools
- Test retrieval priority (tags, links, recency)
- Test memory file format round-trip (write → read → compare)
- Test index consistency after writes

### Consolidation
- Test trigger conditions (all three gates)
- Test Orient phase: correct candidate identification
- Test archive flow: memory → _archived/, never deleted
- Test dry-run mode: no mutations without confirmation

### Profiles
- Test active state update after session end
- Test user profile consolidation from facts
- Test project profile consolidation from facts/patterns/workflows
- Test profile injection into system prompt

---

## 9. Rollout Plan

1. **Sprint 1: Skills System** (smallest, highest leverage)
   - loader.rs + registry.rs + bundled skills
   - SkillTool implementation
   - PromptSection integration

2. **Sprint 2: Prompt System Refactor** (foundation for everything else)
   - PromptSection trait + PromptAssembly
   - Migrate existing system_prompt to new architecture
   - Implement all Dynamic sections (date, git, cwd)

3. **Sprint 3: Memory System** (largest Phase 1 module)
   - format.rs + index.rs + query.rs
   - MemoryRead/Write/Grep/Edit/Status tools
   - Memory injection into prompt

4. **Sprint 4: Consolidation + Profiles** (ties everything together)
   - Consolidation pipeline (Orient → Index; Phase 1 scope)
   - ProfileManager with user/project/active profiles
   - Profile injection into prompt
