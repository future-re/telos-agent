# TUI CLI & Workspace 重构设计
> **Historical note:** This document describes the project state before the
> `telos-cli/` → `cli/` rename and before the root workspace was added.
> Some paths and commands may be outdated.


**日期**: 2026-06-18
**状态**: 设计完成，待实现

## 目标

1. **修复 build 割裂**：将 `telos-cli` 纳入 Cargo workspace，与 core 库统一构建
2. **重塑 CLI 体验**：从"命令行工具"转变为 Codex CLI 风格的交互式 AI 编程搭档，提供全屏 TUI

## 一、Build 修复：Cargo Workspace

### 问题

根 `Cargo.toml` 是单 crate 包，`telos-cli/` 通过 `path = ".."` 松散引用，无法 `cargo build --workspace` 一键构建。README 中声称的 workspace 并不存在。

### 修复

1. 根 `Cargo.toml` 添加 `[workspace]`：
   ```toml
   [workspace]
   resolver = "3"
   members = [".", "cli"]
   ```

2. 目录重命名：`telos-cli/` → `cli/`

3. `cli/Cargo.toml` 调整：
   ```toml
   [package]
   name = "telos-cli"
   # ...
   [dependencies]
   telos_agent = { path = ".." }
   ```

4. 在 workspace root 添加 `[workspace.dependencies]`，集中管理共享依赖版本（`serde`、`tokio`、`tracing` 等）。

5. 更新 CI/README 中的构建命令为 `cargo build --workspace`。

**影响范围**：2 个 `Cargo.toml`、目录重命名、README。风险极低，独立可做。

## 二、TUI 架构

### 入口逻辑

```
telos "do X"          → run_one_shot()    保持
telos completion ...  → run_subcommand()   保持
telos chat            → run_tui()          全屏 TUI（兼容旧 flag）
telos（无参数）        → run_tui()          全屏 TUI（默认）
```

### 技术选型

| 层 | 技术 |
|---|---|
| TUI 框架 | `ratatui`（最新版） |
| 终端控制 | `crossterm` |
| 多行输入 | `tui-textarea` |
| Markdown 渲染 | `termimad`（已用）+ `ratatui` widgets |
| 异步运行时 | `tokio`（已用） |

### 面板布局

```
┌─ StatusBar ────────────────────────────────────────────────────┐
│  telos · deepseek-chat · tiny-agent · main · [session #3]      │
├─ ChatPanel ────────────────────────────────────────────────────┤
│  历史对话（可滚动）                                               │
│  ├─ UserBubble      "refactor error handling..."                │
│  ├─ AgentBubble     streaming markdown 实时渲染                  │
│  ├─ ToolCard        内联工具卡片（名称、状态、耗时、输出摘要）     │
│  └─ DiffOverlay     浮动审批层（syntax-highlighted diff）        │
├─ InputPanel ────────────────────────────────────────────────────┤
│  > _  tui-textarea 多行输入                                    │
│     · Alt+Enter 换行  · Enter 发送  · ↑/↓ 历史                 │
└─────────────────────────────────────────────────────────────────┘
```

### 事件循环

```
crossterm events ─┐
                  ├─→ App::update() ─→ App::render() ─→ ratatui frame
TurnEvent stream ─┘
```

- **App::update**：单一状态更新入口，合并键盘事件、turn 事件、resize 事件
- **App::render**：纯函数，根据当前状态绘制所有 widget
- agent turn 在 tokio task 中执行，通过 `tokio::sync::mpsc` 将 `TurnEvent` 发送给 UI 线程

### 关键集成点

| telos_agent trait/event | TUI 行为 |
|---|---|
| `TurnEvent::AssistantDelta { text }` | 追加到 AgentBubble，markdown 逐字渲染 |
| `TurnEvent::ToolCall { name, id }` | 创建 ToolCard，状态 spinner |
| `TurnEvent::ToolCompleted { name, is_error }` | 更新 ToolCard，✓/✗ |
| `TurnEvent::TurnFinished` | 刷新缓冲区，标记 turn 结束 |
| `ApprovalHandler` | 通过 `tokio::sync::oneshot` 桥接：UI 弹出 DiffOverlay，等待 `a`/`d`/`e` 按键后 resolve |
| `HookRegistry` 事件 | 更新 StatusBar、记录到 SessionMetadata |

### 快捷键

| 键 | 作用 |
|---|---|
| `Enter` | 发送消息 |
| `Alt+Enter` | 输入区换行 |
| `Ctrl+C` | 中断当前 agent 操作 |
| `Ctrl+D` | 退出（空输入时） |
| `↑/↓` | 输入历史（输入区聚焦时）/ 对话滚动（对话区聚焦时） |
| `PgUp/PgDn` | 对话区翻页 |
| `a/d/e` | 审批 DiffOverlay：approve / deny / edit |
| `Ctrl+L` | 清屏 |
| `Ctrl+N` | 新建会话 |
| `Ctrl+Tab` | 切换最近会话 |
| `Ctrl+R` | 搜索历史 |
| `/` | 聚焦搜索栏（对话内搜索） |
| `Tab` | 输入补全（文件路径、slash command、tool 名） |

## 三、会话管理与上下文感知

### 启动流程

```
1. 发现 project_root
   ├─ 从 cwd 向上遍历，查找 .git / .telos.toml
   └─ 找不到则 project_root = cwd

2. 加载配置（merged: CLI flags > project config > user config）
   ├─ ~/.config/telos/config.toml      用户配置
   └─ <project_root>/.telos.toml      项目配置

3. 加载上下文
   ├─ CLAUDE.md / AGENTS.md / CODEBUDDY.md → system prompt section
   ├─ MemoryStore → 相关记忆摘要
   └─ git status → GitStatusSection

4. 恢复或创建 Session
   ├─ 若 --session <name>：加载 <project>/.telos/sessions/<name>.jsonl
   └─ 否则：新建 session（时间戳 + 短描述命名）
   ```

### 上下文注入

启动时自动构建 `PromptAssembly`，包含以下 section：
- `IdentitySection` — agent 身份
- `CwdSection` — 工作目录
- `DateSection` — 当前日期
- `GitStatusSection` — git 状态（有 .git 时）
- `SkillsSection` — 项目 .telos/skills/ + 内置
- `MemorySection` — MemoryStore 相关摘要
- `ProfileSection` — 用户/项目画像
- `McpSection` — MCP 工具列表

**CLAUDE.md** 作为附加 system prompt section 注入，确保模型感知项目约定。

### Session 持久化

- 每次 turn 结束自动调用 `JsonlStorage` 持久化
- 存储位置：`<project_root>/.telos/sessions/<name>.jsonl`
- 无 project_root 时回退到 `~/.local/share/telos/sessions/`
- 恢复时重建完整 `Vec<Message>` 历史

## 四、组件拆分

### crate 结构

```
cli/
├── Cargo.toml
└── src/
    ├── main.rs          入口 + CLI 解析
    ├── lib.rs           模块声明
    ├── cli.rs           clap 定义（保留）
    ├── config.rs        配置加载与合并（保留，增强）
    ├── project.rs       项目根发现（保留）
    ├── session.rs       session 管理（保留，增强）
    ├── approval.rs      终端审批（保留，适配 TUI）
    ├── display.rs       输出渲染（保留，部分迁移到 tui/）
    │
    ├── tui/             新增 TUI 模块
    │   ├── mod.rs
    │   ├── app.rs       App 状态机 + update + render
    │   ├── event.rs     事件循环（crossterm + TurnEvent）
    │   ├── status_bar.rs
    │   ├── chat_panel.rs
    │   ├── tool_card.rs
    │   ├── diff_overlay.rs
    │   ├── input_panel.rs
    │   ├── markdown.rs  ratatui markdown 渲染
    │   └── theme.rs     颜色主题
    │
    ├── runner.rs        保留：one-shot 模式
    └── repl.rs           保留：兼容 telos chat → 转发到 TUI
```

### 状态机（tui/app.rs）

```rust
enum Mode {
    Normal,       // 等待用户输入
    Streaming,    // agent 正在流式输出
    Approving,    // 审批覆盖层显示中
    Searching,    // 搜索模式
    Switching,    // 会话切换模式
}

struct App {
    mode: Mode,
    session: AgentSession,
    provider: Box<dyn ModelProvider>,
    tools: ToolRegistry,
    messages: Vec<UiMessage>,       // 渲染用的消息列表
    input: tui_textarea::TextArea,
    chat_scroll: usize,
    approval_queue: VecDeque<ApprovalRequest>,
    pending_tool_cards: HashMap<String, ToolCardState>,
    status_bar: StatusBarState,
    session_manager: SessionManager,
}
```

### 事件桥接

```rust
// 将 AgentSession::run_turn_stream 产生的 TurnEvent 发送到 UI
let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

// 后台 task
tokio::spawn(async move {
    let mut stream = pin!(session.run_turn_stream(&provider, &tools, prompt));
    while let Some(event) = stream.next().await {
        let _ = tx.send(Event::Turn(event));
    }
    let _ = tx.send(Event::TurnComplete);
});

// UI 线程
while let Some(event) = rx.recv().await {
    app.handle_turn_event(event);
}
```

### 审批桥接

```rust
// TUI 侧实现 ApprovalHandler
struct TuiApprovalHandler {
    tx: tokio::sync::mpsc::UnboundedSender<ApprovalRequest>,
}

impl ApprovalHandler for TuiApprovalHandler {
    async fn request_approval(&self, req: ApprovalRequest) -> ApprovalDecision {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tx.send(req.with_responder(tx))?;
        rx.await.unwrap_or(ApprovalDecision::Deny)
    }
}

// UI 线程：弹出 DiffOverlay，等待按键后 resolve oneshot
```

## 五、测试策略

| 层 | 测试方式 |
|---|---|
| `cli.rs`（参数解析） | 单元测试（已有） |
| `config.rs`（配置合并） | 单元测试（已有） |
| `project.rs`（项目发现） | 单元测试（已有） |
| `tui/app.rs`（状态机） | 单元测试：构造 `App`，注入事件，验证 `mode`/`messages` 状态转换 |
| `tui/widgets` | snapshot 测试 + 手动验证（TUI 层难以完全自动化） |
| 端到端 | `assert_cmd` 集成测试（已有框架），验证 one-shot 和 `--help` 行为不变 |
| TUI 交互 | 手动测试为主，后续可考虑用 `expect`/`tmux` 做自动化 |

## 六、实现顺序

1. **Build 修复**：workspace + 目录重命名 → 验证 `cargo build --workspace && cargo test --workspace`
2. **TUI 骨架**：ratatui 依赖添加 + `tui/app.rs` 基础帧循环 + 空面板布局
3. **事件系统**：crossterm 键盘事件 + TurnEvent 桥接
4. **ChatPanel**：消息列表渲染 + 滚动
5. **输入**：tui-textarea 集成 + 发送逻辑
6. **ToolCard**：工具执行状态展示
7. **DiffOverlay + 审批**：oneshot 桥接审批流
8. **上下文感知**：CLAUDE.md 自动加载 + startup 流程
9. **Session 管理**：新建/恢复/切换
10. **快捷键**：完整键盘映射
11. **清理**：移除不再需要的 rustyline/repl 代码（保留 one-shot 需要的部分）
12. **测试与文档**：更新 README、测试用例

## 七、不变与兼容

- `telos "prompt"` one-shot 模式保持不变
- `telos completion` 子命令保持不变
- `telos chat` CLI flag 保持，行为改为启动 TUI
- core 库（`telos_agent`）无需任何修改 — TUI 完全在 `cli/` 层实现
- 现有 trait（`ModelProvider`、`ApprovalHandler`、`Tool` 等）全部复用
