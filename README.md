# tiny_agent_core

`tiny_agent_core` 是从这个仓库里抽取思路后，用 Rust 重写的一个最小 agent runtime。

它对应原项目里最接近的几层：

- `QueryEngine.ts`: 回合驱动与消息历史
- `Tool.ts`: 工具抽象与调用上下文的核心部分
- `utils/messages.ts`: 消息和工具调用/结果的基本结构
- `bootstrap/state.ts` / `AppState`: 在这里被收敛成会话级 `AgentSession`

这版刻意不包含：

- TUI / React 组件
- MCP / plugin / bridge / swarm
- 权限审批与复杂 hook
- Claude 专用 system prompt 拼装
- Anthropic SDK 协议细节

## 当前能力

- 多轮消息历史
- 模型 provider 抽象
- Anthropic Messages API provider
- 工具注册表
- assistant 发起 tool call
- tool result 回注会话
- 基于 `max_iterations` 的安全停止
- `MockProvider` 测试/样例支撑

## 运行示例

```bash
cargo run --example basic
```

真实模型示例：

```bash
export ANTHROPIC_API_KEY=...
cargo run --example anthropic_tool_loop -- "Use the echo_json tool once, then summarize."
```

## 跑测试

```bash
cargo test
```

## 后续建议

如果你想继续把它往“更完整 agent runtime”推进，下一步最合理的是：

1. 给 `ToolOutput` 和 `Message` 加 streaming/event 模型
2. 把会话持久化从内存扩展到 JSONL / sqlite
3. 单独设计权限层，而不是把原仓库的复杂策略直接搬过来
4. 再补第二个 provider adapter，而不是过早做统一抽象
