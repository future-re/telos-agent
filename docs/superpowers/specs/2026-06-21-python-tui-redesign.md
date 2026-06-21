# Python Textual TUI Redesign

**日期**: 2026-06-21
**状态**: 设计完成，待实现

## 目标

重新设计 `cli/python/` 下的 Python TUI，保留 `telos serve` JSON-line 协议层，用 Textual
reactive 模式驱动 widget 更新，替代当前 app.py 内联 widget、逐行 RichLog 写入的方式。

## 技术选型

| 层 | 技术 | 理由 |
|---|---|---|
| TUI 框架 | Textual >= 0.60 | CSS-like 布局、async workers、reactive 属性、Rich 集成 |
| 后端 | `telos serve` | 已有的 JSON-line daemon，协议不变 |
| 协议 | JSON-line over stdin/stdout | 已实现，保持 |

## 架构

```
┌────────────────────────────────────────────────┐
│                  Python TUI                     │
│  ┌──────────┐  ┌───────────┐  ┌────────────┐  │
│  │ AppState │←─│ EventLoop │←─│  Protocol   │  │
│  │ (store)  │  │ (worker)  │  │  (stdin/out)│  │
│  └────┬─────┘  └───────────┘  └─────┬───────┘  │
│       │ reactive                     │          │
│  ┌────▼──────────────────────────────▼───────┐  │
│  │              Widget Tree                   │  │
│  │  Header │ ChatView │ ToolCard │ Approval │  │
│  │  InputPanel │ StatusBar                   │  │
│  └───────────────────────────────────────────┘  │
└──────────────────────┬─────────────────────────┘
                       │ JSON-line (stdin/stdout)
               ┌───────▼────────┐
               │  telos serve    │
               │  (Rust daemon)  │
               └────────────────┘
```

核心思路：中央 AppState store 通过 Textual reactive 机制驱动 widget 更新。
Event 处理函数只更新 store，不直接操作 widget。

## AppState Store

```python
class AppState:
    # Connection
    connected: reactive[bool] = reactive(False)

    # Streaming
    streaming: reactive[bool] = reactive(False)
    assistant_text: reactive[str] = reactive("")
    thinking_text: reactive[str] = reactive("")

    # Turn tracking
    turn_active: reactive[bool] = reactive(False)
    turn_elapsed: reactive[float] = reactive(0.0)

    # Token usage
    input_tokens: reactive[int] = reactive(0)
    output_tokens: reactive[int] = reactive(0)
    token_budget_max: reactive[int] = reactive(0)
    cost: reactive[float] = reactive(0.0)

    # Tools
    tool_entries: reactive[list[ToolEntry]] = reactive([])

    # Approval
    pending_approval: reactive[Optional[dict]] = reactive(None)

    # Auto mode
    auto_approve: reactive[bool] = reactive(False)

    # Status text
    status_line: reactive[str] = reactive("telos · starting…")
```

- Store 是普通 dataclass，挂在 App 上
- Widget 通过 `watch_*` 方法监听各自关心的字段
- Event 处理函数只更新 store，不直接操作 widget

## Layout

```
┌─ Header ──────────────────────────────────────────────────┐
│  telos  ·  deepseek-chat  ·  [session #3]       🔔 ⏑ ✕   │
├─ ChatView ─────────────────────────────────────────────────┤
│  ┌─ UserBubble ─────────────────────────────────────────┐ │
│  │ refactor the error handling in serve.rs              │ │
│  └──────────────────────────────────────────────────────┘ │
│  ┌─ AssistantBubble ────────────────────────────────────┐ │
│  │ Sure, let me look at the current code...             │ │
│  │ ```rust                                              │ │
│  │ pub fn handle(...                                    │ │
│  │ ```                                                  │ │
│  └──────────────────────────────────────────────────────┘ │
│  ┌─ ToolCard (collapsed) ───────────────────────────────┐ │
│  │ ✓ Read  serve.rs  (120ms)                            │ │
│  └──────────────────────────────────────────────────────┘ │
│  ┌─ ToolCard (expanded) ────────────────────────────────┐ │
│  │ ✓ Read  serve.rs  (120ms)                            │ │
│  │   ┌ output ─────────────────────────────────────┐    │ │
│  │   │ //! `telos serve` — JSON-line daemon mode.   │    │ │
│  │   └─────────────────────────────────────────────┘    │ │
│  └──────────────────────────────────────────────────────┘ │
│  ┌─ AssistantBubble ────────────────────────────────────┐ │
│  │ I'll refactor the approval handler...                │ │
│  └──────────────────────────────────────────────────────┘ │
├─ ApprovalBar (条件显示) ───────────────────────────────────┤
│  ⚠ Approval: Bash  ·  cat src/secret.env                 │
│  [y] Allow  [n] Deny  [m] Modify                          │
├─ InputPanel ───────────────────────────────────────────────┤
│  > _                                                      │
├─ StatusBar ────────────────────────────────────────────────┤
│  ⣾ telos  ·  thinking  ·  3.2s  ·  ↑1.2k ↓0.3k  ·  2 tools │
└────────────────────────────────────────────────────────────┘
```

### Widget Responsibilities

| Widget | 职责 | 监听的 state |
|--------|------|-------------|
| `HeaderWidget` | 品牌、session id、快捷按钮 | — (静态) |
| `ChatView` | 消息列表容器，管理滚动 | — (容器) |
| `MessageBubble` | 单条 user/assistant/thinking/system bubble | — (静态渲染) |
| `ToolCard` | 工具调用卡片，可展开/折叠 | `tool_entries` |
| `ApprovalBar` | 条件显示的审批栏 | `pending_approval` |
| `InputPanel` | 多行输入 + history navigation | — |
| `StatusBar` | 底部状态：spinner/tokens/elapsed/tools | `status_line`, `streaming` 等 |

- ChatView 不用 RichLog 逐行 write，改用 MessageBubble 列表渲染，每条消息有独立样式和交互
- ToolCard 点击/快捷键可展开看 output；不保留独立的 ToolActivityPanel
- ApprovalBar 叠加在输入区上方，条件显示

## Event Flow

### telos serve 事件 → state 更新

```
telos serve stdout
       │
       ▼
  Protocol._read_events()    ← async task, 逐行读 JSON
       │
       ▼
  EventLoop.handle_event()   ← 分发到 handler
       │
       ├─ AssistantDelta   → state.assistant_text += text
       ├─ ThinkingDelta    → state.thinking_text += text
       ├─ ToolCall         → state.tool_entries.push(ToolEntry(...))
       ├─ ToolCompleted    → state.tool_entries[call_id].status = done
       ├─ ToolProgress     → state.tool_entries[call_id].result += msg
       ├─ ProviderUsage    → state.input_tokens, output_tokens...
       ├─ TurnStarted      → state.streaming = True，reset per-turn state
       ├─ TurnFinished     → state.streaming = False，final flush
       ├─ ApprovalRequested → 在 chat 中显示等待审批
       ├─ _approval_required → state.pending_approval = {...}
       ├─ _session_new     → 清空 chat
       ├─ _error           → 显示 error
       └─ _done            → flush，streaming = False
```

### 用户操作 → protocol

```
Input submitted     → {"cmd":"run", "prompt":"..."}
/clear              → 纯客户端清屏
/new                → {"cmd":"new_session"}
/auto               → 切换 state.auto_approve
y (while approval)  → {"cmd":"_approve","decision":"allow"}
n (while approval)  → {"cmd":"_approve","decision":"deny"}
ctrl+c              → {"cmd":"quit"} → 优雅退出
```

## Streaming 处理策略

分三层处理流式文本：

1. **积累** — 将 delta 追加到 buffer，不做渲染
2. **节流** — 每 50ms（或遇到 `\n\n` 段落边界）触发全文 Markdown 渲染
3. **完成** — TurnFinished / _done 时做最终渲染

避免频繁重解析 markdown，也避免被切在 markdown token 中间。

## File Structure

```
cli/python/
├── pyproject.toml
├── README.md                   # 新增
├── telos_tui/
│   ├── __init__.py             # version
│   ├── __main__.py             # 入口 (已有，保持)
│   ├── app.py                  # TelosTuiApp (compose, bindings, mount)
│   ├── state.py                # AppState store (reactive fields)
│   ├── protocol.py             # 已有，保持，微调错误处理
│   ├── event_loop.py           # Event 分发逻辑（从 app.py 抽出）
│   ├── streaming.py            # 流式文本缓冲 + 节流渲染
│   └── widgets/
│       ├── __init__.py         # exports
│       ├── header.py           # HeaderWidget
│       ├── chat_view.py        # 消息列表 + 滚动
│       ├── message_bubble.py   # 单条消息（user/assistant/thinking/system）
│       ├── tool_card.py        # 工具调用卡片（折叠/展开）
│       ├── approval_bar.py     # 审批栏（已有，重构为 watch state）
│       ├── input_panel.py      # 多行输入 + history（已有，微调）
│       └── status_bar.py       # 状态栏（已有，改为 watch state）
```

## 删除的代码

- `app.py` 内联的 `StatusBar` 和 `ApprovalBar` 类
- `app.py` 内 `handle_event` 方法（移到 `event_loop.py`）
- `widgets/tool_activity.py` — ToolActivityPanel（由 ChatView 内的 ToolCard 替代）
- `widgets/chat_area.py` — ChatArea（由 ChatView + MessageBubble 替代）

## 实现顺序

| 阶段 | 内容 | 风险 |
|------|------|------|
| 1. 重构 | 从 app.py 抽出 event_loop、state、streaming；删除内联 widget；接入 widgets/ 中已有组件 | 低 |
| 2. ChatView | 从 RichLog write 模式改为 MessageBubble 列表渲染 | 中 |
| 3. ToolCard | 新增折叠/展开的工具卡片 widget | 低 |
| 4. 集成 | 端到端跑通 telos serve 协议 | 中 |
| 5. 打磨 | Markdown 节流渲染、滚动管理、key bindings 完善 | 低 |

阶段 1 先完成解耦和重组（不引入新 widget），验证协议通信正常。然后逐步替换 ChatView、加 ToolCard。
