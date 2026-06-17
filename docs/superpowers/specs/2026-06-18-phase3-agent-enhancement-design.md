# Phase 3: Agent Enhancement Layer — Design Spec

**Date:** 2026-06-18
**Status:** Design approved
**Scope:** Subagent Fork System + Hooks Enhancement + Task Management
**Dependencies:** Phase 1 (Prompt System for fork system_prompt construction)

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     AgentSession                             │
│                                                              │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐   │
│  │  Fork Engine │  │ Hook System  │  │  Task Manager    │   │
│  │             │  │ (enhanced)   │  │                  │   │
│  │  lens1 ──┐  │  │              │  │  fork → task     │   │
│  │  lens2 ──┤  │  │ PreToolUse   │  │  hook → task     │   │
│  │  lens3 ──┘  │  │ PostToolUse  │  │  track progress  │   │
│  │    ↓        │  │ SessionStart │  │  manage deps     │   │
│  │ synthesize  │  │ ...          │  │                  │   │
│  └─────────────┘  └──────────────┘  └──────────────────┘   │
│                                                              │
│  三者关系：                                                   │
│   - Fork 引擎创建 task 追踪每个 lens 的执行                    │
│   - Hook 触发器可以创建一个 task（异步 hook）                  │
│   - Task 系统为所有后台活动提供统一状态/进度/输出              │
└─────────────────────────────────────────────────────────────┘
```

---

## 1. Subagent Fork System

### 1.1 核心理念

**不做 subprocess，不做 worktree。** Fork 模式在当前 session 内并发执行多个视角（lens），共享所有上下文，只改变 prompt。

```
                ┌──────────────────────┐
                │     Parent Session    │
                │                      │
                │  Provider ◄──────────┤
                │  Tools ◄─────────────┤
                │  Messages (shared) ◄─┤
                │  Memory ◄────────────┤
                │  Config ◄────────────┤
                └──────┬───────────────┘
                       │
         ┌─────────────┼─────────────┐
         │             │             │
    ┌────▼────┐  ┌────▼────┐  ┌────▼────┐
    │ Lens A  │  │ Lens B  │  │ Lens C  │
    │ Security│  │   Perf  │  │Correctns│
    │         │  │         │  │         │
    │ prompt  │  │ prompt  │  │ prompt  │
    │ +schema │  │ +schema │  │ +schema │
    └────┬────┘  └────┬────┘  └────┬────┘
         │             │             │
         └─────────────┼─────────────┘
                       │
              ┌────────▼────────┐
              │   Synthesize    │  (optional)
              └─────────────────┘
```

### 1.2 核心类型

```rust
/// 共享的父 session 状态 — 所有 lens 共用
struct ForkShared {
    provider: Arc<dyn ModelProvider>,      // 共享 HTTP 连接池
    tool_registry: Arc<ToolRegistry>,      // 共享工具
    memory_store: Arc<MemoryStore>,        // 共享记忆
    messages: Arc<RwLock<Vec<Message>>>,   // 共享对话历史（只读）
    config: Arc<AgentConfig>,              // 共享配置
    cwd: PathBuf,
}

/// 单个视角的定义
struct ForkLens {
    /// 视角名称（用于日志/追踪）
    lens: String,                          // "security", "performance", "correctness"
    
    /// 注入到此 lens 的系统提示词
    system_prompt: String,                 // "You are a security auditor..."
    
    /// 具体任务
    task: String,                          // "Review src/auth.rs for injection"
    
    /// 期望的输出格式
    output_schema: Option<Value>,          // JSON Schema for structured output
    
    /// 允许的工具（默认只读 subset）
    allowed_tools: Vec<String>,
    
    /// 可选模型覆盖
    model: Option<String>,                 // 不指定则继承父 session
}

/// 单个 lens 的执行结果
enum ForkResult {
    Text(String),
    Structured(Value),                     // 通过 output_schema 验证
}

/// Fork 执行的完整结果
struct ForkExecution {
    results: Vec<Option<ForkResult>>,      // None = lens 失败
    synthesizer_output: Option<String>,    // 可选的合成输出
    task_ids: Vec<String>,                // 每个 lens 对应的 task ID
}
```

### 1.3 执行引擎

```rust
impl AgentSession {
    /// Fork 多个视角并发执行
    async fn fork_and_synthesize(
        &self,
        lenses: Vec<ForkLens>,
        synthesizer: Option<String>,       // 可选的合成提示词
    ) -> Result<ForkExecution> {
        
        let shared = ForkShared {
            provider: self.provider.clone(),
            tool_registry: self.tool_registry.clone(),
            memory_store: self.memory_store.clone(),
            messages: Arc::new(RwLock::new(self.messages.clone())),
            config: Arc::new(self.config.clone()),
            cwd: self.cwd.clone(),
        };
        
        let synapse = Arc::new(Synapse::new(shared.config.concurrency_limit));
        
        // Phase 1: 并发执行所有 lens
        let results: Vec<Option<ForkResult>> = synapse
            .run_all(lenses, |lens| self.execute_lens(&shared, lens))
            .await;
        
        // Phase 2: 可选合成
        let synthesizer_output = if let Some(prompt) = synthesizer {
            Some(self.synthesize(&shared, &results, prompt).await?)
        } else {
            None
        };
        
        Ok(ForkExecution {
            results,
            synthesizer_output,
            task_ids: vec![], // populated by TaskManager
        })
    }
    
    /// 执行单个 lens
    async fn execute_lens(
        &self,
        shared: &ForkShared,
        lens: ForkLens,
    ) -> Option<ForkResult> {
        // 1. 构建此 lens 的消息
        let fork_messages = vec![
            Message::system(&lens.system_prompt),
            Message::user(&self.build_context_summary(shared)), // 共享上下文摘要
            Message::user(&lens.task),
        ];
        
        // 2. 过滤工具
        let fork_tools = shared.tool_registry
            .filter_by_names(&lens.allowed_tools)
            .definitions();
        
        // 3. 直接调用 provider（一次 complete，不是完整 turn loop）
        let response = shared.provider.complete(CompletionRequest {
            messages: fork_messages,
            tools: fork_tools,
        }).await.ok()?;
        
        // 4. 提取结果
        let text = response.message.text_content();
        
        if let Some(schema) = &lens.output_schema {
            extract_structured_output(&text, schema)
                .map(ForkResult::Structured)
        } else {
            Some(ForkResult::Text(text))
        }
    }
}
```

### 1.4 并发控制 — Synapse

```rust
/// 轻量级并发控制 — 限制同时执行的 lens 数量
struct Synapse {
    semaphore: Arc<Semaphore>,
}

impl Synapse {
    fn new(max_concurrent: usize) -> Self {
        Synapse {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }
    
    async fn run_all<T, F, Fut>(
        &self,
        items: Vec<T>,
        f: F,
    ) -> Vec<Option<Fut::Output>>
    where
        F: Fn(T) -> Fut,
        Fut: Future,
    {
        futures::future::join_all(
            items.into_iter().map(|item| {
                let permit = self.semaphore.clone().acquire_owned().await.unwrap();
                let result = f(item).await;
                drop(permit);
                result
            })
        ).await
    }
}
```

### 1.5 上下文摘要构建

```rust
impl AgentSession {
    /// 为 fork lens 构建共享上下文摘要
    /// 不直接传递所有 messages（会超 context），而是做精简摘要
    fn build_context_summary(&self, shared: &ForkShared) -> String {
        let mut parts = Vec::new();
        
        // 1. 项目画像（始终包含）
        if let Some(profile) = &self.cached_profile {
            parts.push(profile.clone());
        }
        
        // 2. 父 agent 的最新请求
        let recent_messages: Vec<&Message> = shared.messages.read().unwrap()
            .iter()
            .rev()
            .take(4)  // 最近 4 条消息（user + assistant + user + assistant）
            .collect();
        
        if !recent_messages.is_empty() {
            parts.push("## Recent Conversation".to_string());
            for msg in recent_messages.iter().rev() {
                let text = msg.text_content();
                if !text.is_empty() {
                    parts.push(format!("{}: {}", msg.role, truncate(&text, 2000)));
                }
            }
        }
        
        // 3. 相关记忆
        if let Some(memories) = &self.relevant_memories {
            parts.push("## Relevant Memories".to_string());
            parts.push(memories.clone());
        }
        
        parts.join("\n\n")
    }
}
```

### 1.6 AgentTool 接口

```rust
// AgentTool 的 fork 模式 input schema
{
    "type": "object",
    "properties": {
        "lenses": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "lens":          { "type": "string" },
                    "system_prompt": { "type": "string" },
                    "task":          { "type": "string" },
                    "output_schema": { "type": "object" },
                    "allowed_tools": { 
                        "type": "array", 
                        "items": { "type": "string" },
                        "default": ["Read", "Grep", "Glob"]
                    }
                },
                "required": ["lens", "system_prompt", "task"]
            }
        },
        "synthesizer": { "type": "string" }
    },
    "required": ["lenses"]
}
```

### 1.7 性能特征

| 指标 | Fork 模式 |
|------|-----------|
| 启动延迟 | **< 5ms**（无进程 spawn，无 worktree 创建） |
| 内存开销 | **~100KB per lens**（只有 prompt + task 字符串） |
| 并发模型 | tokio::spawn，共享 provider 连接池 |
| 默认并发上限 | 10（继承 AgentConfig.concurrency_limit） |
| 上下文共享 | 摘要形式，不复制全部 messages |

---

## 2. Hooks Enhancement

### 2.1 当前 vs 目标

| 能力 | 当前 | 目标 |
|------|:---:|:---:|
| PostSampling | ✅ | ✅ |
| Stop | ✅ | ✅ |
| PreToolUse | ❌ | ✅ |
| PostToolUse | ❌ | ✅ |
| PostToolUseFailure | ❌ | ✅ |
| SessionStart | ❌ | ✅ |
| UserPromptSubmit | ❌ | ✅ |
| 条件过滤 | ❌ | ✅ if 条件 |
| 多种 hook 类型 | ❌ | ✅ Command / Prompt / Http |
| Async execution | ❌ | ✅ |

### 2.2 新的 HookPhase

```rust
enum HookPhase {
    // 现有
    PostSampling,                               // 每次 LLM 采样后
    Stop,                                       // turn 结束时

    // 新增
    PreToolUse { tool_name: String },           // 工具执行前
    PostToolUse { tool_name: String },          // 工具执行后（成功）
    PostToolUseFailure { tool_name: String },   // 工具执行后（失败）
    SessionStart,                               // AgentSession 创建时
    UserPromptSubmit,                           // 用户提交提示词后
}
```

### 2.3 HookType 扩展

```rust
enum HookType {
    /// 执行 shell 命令（当前唯一类型）
    Command {
        command: String,
        shell: Option<String>,
    },
    
    /// 🆕 LLM 提示词（用小模型，不占主 context）
    Prompt {
        prompt: String,
    },
    
    /// 🆕 HTTP 调用
    Http {
        url: String,
        method: HttpMethod,
        headers: HashMap<String, String>,
        body: Option<String>,
    },
}

enum HttpMethod { Get, Post, Put, Delete }
```

### 2.4 条件过滤

```rust
struct HookCondition {
    /// 工具名称匹配
    tool_name: Option<String>,         // "Bash", "Bash(git *)", "*"
}

struct HookEntry {
    hook: Arc<dyn Hook>,
    phase: HookPhase,
    condition: Option<HookCondition>,  // None = 始终触发
    once: bool,                        // 触发一次后自动移除
    async_exec: bool,                  // 异步执行，不阻塞主流程
}
```

### 2.5 执行器选择

```rust
impl HookRegistry {
    async fn run_hooks(
        &self,
        phase: HookPhase,
        ctx: &HookContext,
    ) -> Vec<HookResult> {
        let hooks = self.hooks_for_phase(&phase);
        
        let results = Vec::new();
        for entry in hooks {
            // 1. 条件匹配
            if !self.matches_condition(&entry, &phase, ctx) {
                continue;
            }
            
            // 2. 执行
            let result = match &entry.hook.hook_type {
                HookType::Command { command, shell } => {
                    self.exec_command(command, shell, ctx).await
                }
                HookType::Prompt { prompt } => {
                    self.exec_prompt(prompt, ctx).await       // 🆕
                }
                HookType::Http { url, method, headers, body } => {
                    self.exec_http(url, method, headers, body).await  // 🆕
                }
            };
            
            results.push(result);
            
            // 3. once 标记
            if entry.once {
                // 标记为已执行，下次移除
            }
        }
        
        results
    }
}
```

### 2.6 模块结构

```
src/hooks/
  mod.rs            — Hook trait + HookRegistry (enhanced)
  types.rs          — HookPhase, HookType, HookCondition, HookEntry, HookContext
  command.rs        — Command hook 执行器
  prompt.rs         — 🆕 Prompt hook 执行器（用 provider 跑小 prompt）
  http.rs           — 🆕 HTTP hook 执行器
  condition.rs      — 🆕 条件匹配引擎
```

---

## 3. Task Management

### 3.1 为什么需要

Task 系统是 Fork 和 Hook 的桥梁：

- 每个 Fork lens → 创建一个 Task（可追踪进度和状态）
- 异步 Hook → 创建 Task（不阻塞主流程）
- 用户可以通过 TaskList 看到所有进行中的活动
- 父 agent 通过 TaskOutput 获取子任务结果

### 3.2 核心类型

```rust
struct Task {
    id: String,                    // UUID
    subject: String,               // "Code review - security lens"
    description: String,
    status: TaskStatus,
    owner: Option<String>,         // agent/lens name
    blocks: Vec<String>,          // task IDs this task blocks
    blocked_by: Vec<String>,      // task IDs blocking this task
    output: Option<TaskOutput>,    // populated on completion
    created_at: Instant,
    updated_at: Instant,
}

enum TaskStatus {
    Pending,                       // not yet started
    InProgress,
    Completed,
    Deleted,                       // soft delete
}

struct TaskOutput {
    content: String,               // text output
    structured: Option<Value>,     // if output_schema was specified
    error: Option<String>,         // if failed
}
```

### 3.3 工具集（模型可见）

| 工具 | 用途 |
|------|------|
| **TaskCreate** | 创建新任务 |
| **TaskGet** | 获取任务详情（含输出） |
| **TaskList** | 列出所有任务（状态摘要） |
| **TaskUpdate** | 更新状态/依赖/owner |

### 3.4 Task 生命周期

```
TaskCreate(id, subject, status=Pending)
             │
    blocked_by 非空 → 等待依赖完成
             │
    依赖满足 → InProgress (owner takes it)
             │
        ┌────┴────┐
        │         │
    Completed   Failed → TaskUpdate(status=Pending) 重试
```

### 3.5 与 Fork 的集成

```rust
impl AgentSession {
    async fn fork_and_synthesize(
        &self,
        lenses: Vec<ForkLens>,
        synthesizer: Option<String>,
    ) -> Result<ForkExecution> {
        // 1. 为每个 lens 创建 task
        let task_ids: Vec<String> = lenses.iter().map(|lens| {
            self.task_manager.create(TaskCreate {
                subject: format!("Fork: {}", lens.lens),
                description: lens.task.clone(),
                status: TaskStatus::Pending,
            })
        }).collect();
        
        // 2. 并发执行，更新 task 状态
        let results = synapse.run_all(lenses.into_iter().enumerate(), |(i, lens)| {
            let task_id = task_ids[i].clone();
            async move {
                self.task_manager.update(task_id, TaskStatus::InProgress).await;
                let result = self.execute_lens(&shared, lens).await;
                let status = if result.is_some() { TaskStatus::Completed } else { TaskStatus::Deleted };
                self.task_manager.update(task_id, status).await;
                result
            }
        }).await;
        
        ForkExecution { results, synthesizer_output: None, task_ids }
    }
}
```

### 3.6 持久化

```
.tiny-agent/tasks/
  task_a1b2c3d4.json
  task_e5f6g7h8.json
```

每个 task 一个 JSON 文件，简单可靠。Session 结束时清理已完成的 task（可选：保留到下次 session）。

### 3.7 模块结构

```
src/tasks/
  mod.rs            — TaskManager
  task.rs           — Task + TaskStatus + TaskOutput
  tool.rs           — TaskCreate/Get/List/Update tools
  persistence.rs    — JSON file persistence
```

---

## 4. Integration Example

```
User: "Review my PR for all issues"

AgentSession:
  │
  ├─ AgentTool invoked with fork mode:
  │   lenses: [
  │     { lens: "security",  task: "review for vulnerabilities" },
  │     { lens: "performance", task: "review for bottlenecks" },
  │     { lens: "correctness", task: "review for logic errors" },
  │   ]
  │   synthesizer: "Merge findings, deduplicate, rank by severity"
  │
  ├─ TaskManager creates 3 tasks: [t1, t2, t3]
  │
  ├─ Synapse runs all 3 lenses concurrently:
  │   t1: Read + Grep → finds injection → TaskUpdate(Completed)
  │   t2: Read only → finds N+1 query → TaskUpdate(Completed)
  │   t3: Read + Grep → finds edge case bug → TaskUpdate(Completed)
  │
  ├─ Synthesizer merges results
  │
  └─ Agent presents findings to user
```

---

## 5. Dependencies

No new dependencies in Phase 3. All features use existing tokio primitives (Semaphore, spawn, JoinSet).

---

## 6. Testing Strategy

### Fork Engine
- Unit: ForkLens schema validation
- Unit: Synapse concurrency limit enforcement
- Integration: 3 lenses execute concurrently, all return results
- Integration: lens failure (one panics) → others continue → synthesis still works
- Integration: structured output extraction with valid and invalid JSON

### Hooks
- Unit: HookCondition matching (tool name wildcards)
- Unit: once flag auto-removal
- Integration: PreToolUse hook runs before tool execution
- Integration: PostToolUseFailure hook receives error info
- Integration: async hook doesn't block main turn

### Task Manager
- Unit: CRUD operations
- Unit: blocked_by dependency resolution
- Integration: Fork creates tasks, tasks update during execution
- Integration: TaskList returns correct status summary

---

## 7. File Layout After Phase 3

```
tiny-agent-core/
├── src/
│   ├── subagent/
│   │   ├── mod.rs              — SubagentTool (fork mode only)
│   │   ├── fork.rs             — 🆕 ContextFork engine + Synapse
│   │   ├── types.rs            — ForkLens, ForkShared, ForkResult, ForkExecution
│   │   └── lens.rs             — 🆕 Individual lens execution + context summary
│   ├── hooks/
│   │   ├── mod.rs              — Hook trait + HookRegistry
│   │   ├── types.rs            — HookPhase, HookType, HookCondition, HookEntry
│   │   ├── command.rs          — Command hook executor
│   │   ├── prompt.rs           — 🆕 Prompt hook executor
│   │   ├── http.rs             — 🆕 HTTP hook executor
│   │   └── condition.rs        — 🆕 Condition matching
│   ├── tasks/
│   │   ├── mod.rs              — 🆕 TaskManager
│   │   ├── task.rs             — 🆕 Task + TaskStatus + TaskOutput
│   │   ├── tool.rs             — 🆕 TaskCreate/Get/List/Update
│   │   └── persistence.rs      — 🆕 JSON file persistence
│   ├── mcp/                    — Phase 2
│   ├── skills/                 — Phase 1
│   ├── prompt/                 — Phase 1
│   ├── memory/                 — Phase 1
│   ├── tool/mod.rs             — MODIFIED: register Fork/Task tools
│   └── runtime.rs              — MODIFIED: hook dispatch, task notifications
└── docs/superpowers/specs/
    ├── 2026-06-18-phase1-core-intelligence-design.md
    ├── 2026-06-18-phase2-extension-layer-design.md
    └── 2026-06-18-phase3-agent-enhancement-design.md
```

---

## 8. Summary

Phase 3 adds three subsystems, all built on existing infrastructure:

| System | New Files | Key Insight |
|--------|-----------|-------------|
| Fork Engine | 4 files | Shared context, divergent prompts, concurrent execution |
| Hooks Enhanced | 3 new files + 2 modified | More lifecycle points, more hook types, conditional execution |
| Task Manager | 4 files | Unifies fork and hook activity tracking with CRUD + dependencies |

**No new dependencies.** Everything built on tokio, serde_json, and existing types.
