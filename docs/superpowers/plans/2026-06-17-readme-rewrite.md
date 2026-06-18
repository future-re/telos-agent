# README 全面重写实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `README.md` 更新为准确、完整、结构清晰的项目入口文档，反映代码库已实现的功能、设计与架构。

**Architecture:** 按「简介 → 功能 → 架构 → 流程 → 对象 → 示例 → 测试 → 边界」组织内容；功能按子系统分组；示例代码须与当前 API 保持一致并可编译。

**Tech Stack:** Markdown、Rust（用于验证示例代码）。

---

### 文件结构

- **修改：** `README.md` — 项目入口文档。
- **参考：** `docs/superpowers/specs/2026-06-17-readme-rewrite-design.md` — 设计文档。
- **参考：** 审计报告（由 explore agent 产出）—— 能力、对象、流程、示例修正要点。

---

### Task 1: 重写简介、定位、功能特性

**Files:**
- Modify: `README.md`（全文替换的开始部分）

**目标：** 更新项目简介、定位，并按子系统分组重写「功能特性」。

- [ ] **Step 1: 写入 README 开头至「功能特性」末尾**

将以下内容写入 `README.md`，覆盖原有开头与「已实现能力」章节：

```markdown
# tiny_agent_core

`tiny_agent_core` 是一个用 Rust 编写的轻量级 agent runtime，重点覆盖会话管理、模型调用、工具执行和结果回注这一条核心链路。它把一次「用户输入 → 模型采样 → 工具执行 → 结果回注」的完整 turn 封装成可扩展、可观测、可持久化的运行时单元。

## 定位与使用场景

它主要面向两类使用场景：

- **作为实验底座**，用于验证 agent loop、tool use、provider adapter、权限审批、memory、skills 等设计。
- **作为运行时内核**，供 TUI、HTTP 服务、任务系统或其他编排层集成；上层只需实现交互界面，核心链路由 `tiny_agent_core` 提供。

## 功能特性

### 核心运行时

- 会话级 `AgentSession`，驱动每一轮 turn 的模型采样与工具执行循环。
- `TurnEvent` 事件流，暴露采样、工具执行、compaction、停止等关键阶段。
- `HookRegistry`，支持 `post_sampling` 和 `stop` 两个 hook phase。
- 基于 `Arc<AtomicBool>` 的取消检查点，可在 provider 调用或迭代间隙安全中断。
- `SessionMetrics` 汇总每轮 turn 的 token、tool call、错误、迭代、compaction、重试等数据。

### Provider 适配

- 通用 `ModelProvider` 抽象，接收消息与工具定义，返回 assistant message。
- 内置 `KimiProvider`、`DeepSeekProvider`、`MockProvider`。
- `ProviderEvent` 流式事件抽象；非流式 provider 可通过默认实现兼容。
- 基于 `async-openai` 的 SSE streaming。
- `RetryConfig` 为单个 provider 提供带指数退避的 transient 错误重试。
- `ErasedProvider` 辅助，使 `Arc<dyn ModelProvider>` 可与 `run_turn_stream` 一起使用。

### 工具系统

- `Tool` trait 覆盖 definition、validate、permission 和 invoke。
- `ToolRegistry` 管理工具注册、别名（`Tool::aliases`）和 JSON Schema 预编译。
- 开启 `auto_validate_schema` 后，运行时自动校验工具参数是否符合 `input_schema`。
- 内置核心工具：`Bash`/`Read`/`Edit`/`Write`/`Glob`/`Grep`（同时保留 `shell`、`file_read` 等旧别名）。
- `SubagentTool`，将 in-process 子 agent 注册为可调用工具。
- `SkillTool`，把加载的 skill 暴露为工具。
- 5 个 Memory 工具：`MemoryRead`、`MemoryWrite`、`MemoryGrep`、`MemoryEdit`、`MemoryStatus`。
- 工具超时（`tool_timeout_ms`）与单工具 panic 隔离。
- 文件写冲突保护：`FileReadState`/`FileReadRecord` 跟踪文件内容和时间戳，拒绝外部修改后的编辑。
- 最大文件读取字节限制（`max_file_read_bytes`），防止大文件导致 OOM。

### 权限与审批

- `PermissionEngine` 规则引擎，支持按工具名通配、`command_prefix`、`cwd_prefix` 等规则判定。
- Bash AST 安全分析（`bash_security` 模块），fail-closed 分类，供 `ShellTool` 与权限引擎使用。
- `ApprovalHandler` 人机审批：`ApprovalRequest` / `ApprovalDecision`（Allow / Deny / Modify）。
- 工具自身 `check_permission` 作为规则引擎的补充 fallback。

### Prompt 与 Skills

- `PromptAssembly` / `PromptSection` 在 turn 时组装 system prompt，支持静态片段缓存。
- 内置 prompt section：`IdentitySection`、`ToolsSection`、`DateSection`、`CwdSection`、`SkillsSection`、`GitStatusSection`。
- Skills 系统：Markdown + YAML frontmatter 形式的 slash-command，注入到 prompt 并可通过 `SkillTool` 调用。
- `SkillRegistry` / `SkillLoader` 负责 skill 的加载、解析和注册。

### Memory

- `MemoryStore` 提供跨会话的持久化记忆，按 `MemoryCategory` 组织为 markdown 文件。
- `MemoryEntry` / `MemoryFormat` / `MemoryStatus` 管理记忆内容、格式与状态。

### 存储与恢复

- `Storage` trait 抽象持久化后端。
- `JsonlStorage` 以 JSONL 形式保存会话快照，支持会话恢复。
- `NoopStorage` 用于不需要持久化的场景。
- `SessionMetadata` 记录会话元数据。

### Compaction 与预算

- `TokenBudget` 基于估算 token 触发 `compact` 或 `budget_exceeded` 事件。
- `CompactionStrategy` / `SummaryCompaction` / `CompactionConfig` 提供可配置的历史压缩策略。
- 字符预算 compaction：单条 tool result 超过 `max_tool_result_chars` 时自动截断预览。

### Streaming 与事件

- `AgentSession::run_turn_stream` 是底层流式 API，按顺序产出 `TurnEvent`。
- `AgentSession::run_turn` 是便利封装，收集所有事件并返回 `TurnResult`。
- `execute_tool_calls_stream` 直接暴露工具执行流水线的事件流。
- `ThinkingBlock` / `ThinkingDelta` 已支持 reasoning 内容的采集与回传。
```

- [ ] **Step 2: 检查格式**

Run: `cat README.md | head -n 120`
Expected: 文件以 `# tiny_agent_core` 开头，包含「定位与使用场景」和完整的「功能特性」分组列表，无原「已实现能力」标题。

- [ ] **Step 3: Commit**

```bash
cd tiny_agent_core
git add README.md
git commit -m "docs: rewrite intro, positioning and features"
```

---

### Task 2: 重写架构概览与执行流程

**Files:**
- Modify: `README.md`

**目标：** 用分层描述替换旧的「执行流程」，并补充详细的执行阶段。

- [ ] **Step 1: 写入「架构概览」与「执行流程」**

在 Task 1 写入的内容之后，追加以下章节：

```markdown
## 架构概览

运行时由几条清晰的分层职责构成：

- **Session 层**：`AgentSession` 持有消息历史、配置、文件读状态、metrics，并对外暴露 `run_turn` / `run_turn_stream`。
- **Runtime 层**：负责单轮 turn 内的迭代循环、provider 调用、compaction、hook 调用、tool 执行编排和持久化。
- **Provider 层**：`ModelProvider` 将不同服务商封装为统一的采样接口；流式输出统一为 `ProviderEvent`。
- **Tool 层**：`Tool` trait + `ToolRegistry` + 执行器，完成参数校验、权限判定、审批、调用和结果格式化。
- **Prompt 层**：`PromptAssembly` 在每次采样前动态组装 system prompt，并缓存静态 section。
- **权限与安全层**：`PermissionEngine` 规则引擎 + `bash_security` AST 分析 + `ApprovalHandler` 人工审批。
- **Skills / Memory 层**：提供可注入的 slash-command 技能和跨会话持久记忆。
- **Storage 层**：`Storage` trait 将会话状态持久化到 JSONL 等后端。

## 执行流程

一次 `run_turn` 内部按如下流水线执行：

1. **Prompt 构建**：如果配置了 `prompt_assembly`，则调用其 `build()` 生成 system prompt；否则使用 `base_system_prompt` 构造默认 identity section。
2. **Compaction 阶段**：每轮迭代开始时检查 token 预算与字符预算，必要时触发 `CompactionStrategy` 压缩历史，避免超出上下文窗口。
3. **Provider 采样**：通过 `ModelProvider::complete` 或流式接口采样；调用被 `RetryConfig` 包裹，支持指数退避重试，并响应 `cancelled` 标志。
4. **Assistant 消息追加**：将模型返回的 assistant message 加入历史，触发 `post_sampling` hooks。
5. **Tool 调用判定**：如果模型未请求工具，进入 stop 流程；否则逐条处理 tool call。
6. **Tool 执行流水线**（对每条 tool call）：
   - 调用 `Tool::validate` 进行工具级校验；
   - 若开启 `auto_validate_schema`，用预编译 JSON Schema 再次校验参数；
   - `PermissionEngine` 按规则判定（含 bash 命令前缀分析）；
   - 兜底调用 `Tool::check_permission`；
   - 若判定为 `Ask`，通过 `ApprovalHandler` 等待人工决策；
   - 在 `tool_timeout_ms` 限制内调用 `Tool::invoke`；
   - 任一阶段失败都会生成 `is_error: true` 的 `ToolResult`。
7. **并发编排**：`execute_tool_calls_stream` 按并发安全性分批执行；可并发工具在 `tool_concurrency_limit` 内并行，结果按声明顺序重组。
8. **结果回注**：对超长 tool result 进行字符预算截断，然后以 `Role::Tool` 消息写回会话。
9. **Stop 判定**：当没有待处理 tool call 时，触发 `stop` hooks，整理 `TurnResult`。
10. **持久化与回滚**：出错时回滚消息、turn ID、metrics 和 `read_file_state`；正常结束时通过 `storage` 持久化，`save_error` 会随 `TurnResult` 返回而不掩盖 turn 结果。
```

- [ ] **Step 2: 检查格式**

Run: `grep -n "## 架构概览\|## 执行流程" README.md`
Expected: 两行均存在，且中间包含上述 10 步流程。

- [ ] **Step 3: Commit**

```bash
cd tiny_agent_core
git add README.md
git commit -m "docs: rewrite architecture overview and execution flow"
```

---

### Task 3: 重写核心对象表格与最小示例

**Files:**
- Modify: `README.md`

**目标：** 用表格形式更新核心对象，并修正最小示例代码使其可编译。

- [ ] **Step 1: 写入「核心对象」与「最小示例」**

在 Task 2 内容之后追加：

```markdown
## 核心对象

| 类型 / Trait | 职责 |
|---|---|
| `AgentSession` | 保存消息历史，驱动每一轮 turn。 |
| `AgentConfig` | 会话配置，包括 `base_system_prompt`、`max_iterations`、`cwd`、`env`、取消标志、重试、权限、存储等扩展点。 |
| `ModelProvider` | 模型适配接口，接收消息和工具定义，返回 assistant message 或事件流。 |
| `CompletionRequest` / `CompletionResponse` / `ProviderEvent` | Provider 输入输出与流式事件抽象。 |
| `Tool` | 工具接口，覆盖 definition、validate、permission 和 invoke。 |
| `ToolRegistry` | 注册工具、管理别名、预编译 JSON Schema。 |
| `ToolContext` / `ToolProgress` | 工具调用时的上下文与进度上报。 |
| `PermissionEngine` / `PermissionRule` / `RuleDecision` | 规则化权限判定。 |
| `ApprovalHandler` / `ApprovalRequest` / `ApprovalDecision` | 人机审批基础设施。 |
| `bash_security` | Bash AST 安全分析。 |
| `HookRegistry` / `Hook` | 在 `post_sampling` 和 `stop` 阶段插入自定义逻辑。 |
| `TokenBudget` | 基于估算 token 触发 compaction 或 budget exceeded。 |
| `CompactionStrategy` / `SummaryCompaction` / `CompactionConfig` | 历史压缩策略。 |
| `PromptAssembly` / `PromptSection` | system prompt 动态装配与静态 section 缓存。 |
| `Skill` / `SkillArg` / `SkillRegistry` / `SkillLoader` / `SkillTool` | Markdown skill 的加载、注册与调用。 |
| `MemoryStore` / `MemoryCategory` / `MemoryEntry` / `MemoryFormat` / `MemoryStatus` | 跨会话持久记忆。 |
| `Storage` / `JsonlStorage` / `NoopStorage` / `SessionMetadata` | 会话持久化与恢复。 |
| `SessionMetrics` | 汇总 token、tool call、错误、迭代等运行时指标。 |
| `TurnEvent` / `TurnResult` / `StopReason` | turn 事件流、结果与停止原因。 |
| `Message` / `ContentBlock` / `TextBlock` / `ThinkingBlock` / `ToolCall` / `ToolResult` | 消息与内容块模型。 |
| `ErasedProvider` | 类型擦除 provider 辅助。 |

## 最小示例

```rust
use async_trait::async_trait;
use serde_json::{json, Value};
use tiny_agent_core::{
    AgentConfig, AgentError, AgentSession, CompletionResponse, Message,
    MockProvider, StopReason, TokenUsage, Tool, ToolContext, ToolDefinition,
    ToolOutput, ToolRegistry,
};

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".into(),
            description: "Echo input text.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                },
                "required": ["text"]
            }),
        }
    }

    async fn invoke(
        &self,
        arguments: Value,
        _context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        Ok(ToolOutput {
            content: json!({ "echo": arguments }),
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), AgentError> {
    let provider = MockProvider::new(vec![CompletionResponse {
        message: Message::assistant("done"),
        stop_reason: StopReason::EndTurn,
        usage: None,
    }]);

    let mut tools = ToolRegistry::new();
    tools.register(EchoTool);

    let mut session = AgentSession::new(AgentConfig {
        base_system_prompt: Some("You are a concise assistant.".into()),
        ..AgentConfig::default()
    }).unwrap();

    let result = session.run_turn(&provider, &tools, "hello").await?;
    println!("{}", result.final_message.text_content());
    Ok(())
}
```
```

- [ ] **Step 2: 验证最小示例可编译**

Run: `cd tiny_agent_core && cargo check`
Expected: 编译通过，无 `AgentConfig` 字段或 `CompletionResponse` 字段错误。

- [ ] **Step 3: Commit**

```bash
cd tiny_agent_core
git add README.md
git commit -m "docs: rewrite core objects table and fix minimal example"
```

---

### Task 4: 补充运行示例、测试与「暂不包含」

**Files:**
- Modify: `README.md`

**目标：** 保留并更新运行示例与测试章节，重写「暂不包含」列表。

- [ ] **Step 1: 写入剩余章节**

在 Task 3 内容之后追加：

```markdown
## 运行示例

仓库中提供了一个基于真实 provider 的工具调用示例：

```bash
export MOONSHOT_API_KEY=...
cargo run --example kimi_tool_loop -- "Use the echo_json tool once, then summarize."
```

## 测试

```bash
cargo test
```

## 暂不包含

以下能力在 `tiny_agent_core` 当前范围之外：

- UI / TUI / Web 层（只提供运行时内核）。
- MCP / plugin / bridge / swarm 等外部扩展协议。
- 多模态输入输出。
- 跨 provider fallback（当前仅支持单 provider 内的重试）。
- 真正的沙箱级执行环境（当前提供的是规则权限引擎 + bash AST 分析 + 人工审批）。
```

- [ ] **Step 2: 检查「暂不包含」准确性**

Run: `grep -n "thinking block\|classifier" README.md`
Expected: 无匹配；确认 thinking block 与 classifier 不再出现在「暂不包含」中。

- [ ] **Step 3: 全文检查**

Run: `cd tiny_agent_core && cargo check`
Expected: 编译通过。

Run: `grep -n "system_prompt:" README.md`
Expected: 不再出现旧的 `system_prompt:` 字段用法（最小示例中应为 `base_system_prompt:`）。

- [ ] **Step 4: Commit**

```bash
cd tiny_agent_core
git add README.md
git commit -m "docs: add run example, tests and update out-of-scope section"
```

---

### Task 5: 最终审阅与收尾

**Files:**
- Modify: `README.md`

**目标：** 通读全文，修正格式、链接、错别字，确保与设计文档一致。

- [ ] **Step 1: 通读全文并修正**

Run: `cat README.md`
检查项：
- 所有 Markdown 标题层级正确（`#`、`##`、`###`）。
- 代码块语言标记正确（`rust`、`bash`）。
- 无拼写错误、无断句问题。
- 功能列表不夸大，与审计报告一致。
- 核心对象表格无遗漏重要类型。

- [ ] **Step 2: 运行项目测试**

Run: `cd tiny_agent_core && cargo test`
Expected: 全部通过（README 修改本身不影响编译，但需确认项目整体健康）。

- [ ] **Step 3: 最终提交**

```bash
cd tiny_agent_core
git add README.md
git commit -m "docs: final README review and polish"
```

---

## Self-Review Checklist

- [ ] Spec coverage：设计文档中的每个章节在新 README 中均有对应任务。
- [ ] Placeholder scan：计划中没有 TBD、TODO、"implement later"、"similar to Task N" 等占位符。
- [ ] Type consistency：`AgentConfig::base_system_prompt`、`CompletionResponse::usage`、`TokenUsage` 等字段名称与源码一致。
- [ ] 示例可编译：Task 3 明确用 `cargo check` 验证最小示例。
