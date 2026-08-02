# telos

[![License](https://img.shields.io/github/license/future-re/telos-agent?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-workspace-orange?style=flat-square&logo=rust)](Cargo.toml)
[![CLI](https://img.shields.io/badge/CLI-telos-blue?style=flat-square)](cli/README.md)
[![Last commit](https://img.shields.io/github/last-commit/future-re/telos-agent?style=flat-square)](https://github.com/future-re/telos-agent/commits/main)

**telos** 是一个 Rust 编写的意图驱动 agent runtime，封装“用户输入 → 模型采样 → 工具执行 → 结果回注”的完整 turn 循环，可作为编码助手、聊天应用和自动化工作流的内核。

> Loop: intent → execute → think → complete

| 模块 | 说明 |
| --- | --- |
| [`telos_agent`](runtime/README.md) | Runtime、provider、工具、权限、memory、MCP、插件和存储 |
| [`telos-cli`](cli/README.md) | 全屏 TUI 与命令行客户端 |
| [`desktop/`](desktop/) | 复用同一 Rust host 的 Tauri 桌面客户端 |
| [`site/`](site/) | 使用文档与项目站点 |

项目当前处于 Alpha 阶段。公共 API、插件 manifest 和本地状态格式仍可能发生主版本变化。

## 快速开始

### 安装 CLI

```bash
# Python 用户
pip install telos-cli

# 或从 crates.io 安装
cargo install telos-cli

telos --help
```

### 运行

```bash
export DEEPSEEK_API_KEY=sk-...

# 全屏 TUI
telos

# 单次调用
telos --provider deepseek --model deepseek-v4-pro "Review src/lib.rs"

# thinking / execution 双模型路由
telos --provider deepseek \
  --thinking-model deepseek-v4-pro \
  --fast-model deepseek-v4-flash \
  "Refactor error handling"
```

不提供 API key 时可使用 `--provider mock` 验证本地流程。

### 作为 Rust 库使用

```rust
use std::sync::Arc;
use telos_agent::{
    AgentConfig, AgentRuntime, CompletionResponse, Message, MockProvider,
    StopReason, ToolRegistry,
};

# #[tokio::main]
# async fn main() -> Result<(), telos_agent::AgentError> {
let provider = Arc::new(MockProvider::new(vec![CompletionResponse {
    message: Message::assistant("done"),
    stop_reason: StopReason::EndTurn,
    usage: None,
    model: None,
}]));
let runtime = AgentRuntime::new(AgentConfig::default(), provider, ToolRegistry::new())?;
let session = runtime.create_session().await?;
let result = runtime.run_turn(&session, "hello").await?;
assert_eq!(result.final_message.text_content(), "done");
# Ok(())
# }
```

## 文档

- [在线使用文档](https://future-re.github.io/telos-agent/)
- [Runtime API 与示例](runtime/README.md)
- [CLI 使用说明](cli/README.md)
- [插件与 MCP](site/src/content/docs/docs/plugins-mcp.mdx)
- [桌面客户端](site/src/content/docs/docs/desktop-client.mdx)
- [变更记录](CHANGELOG.md)

插件管理已收敛到 manifest v3：marketplace 和插件来源只支持本地目录与 GitHub，依赖只在同一 marketplace 内解析，所有管理操作通过项目级锁串行执行。旧 v2 manifest 和旧状态文件不会被自动迁移或删除。

## 开发

```bash
# Rust workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Desktop frontend
cd desktop
npm ci
npm test
npm run build

# Documentation site
cd ../site
npm ci
npm run check
```

本地启动桌面端使用 `cd desktop && npm run tauri dev`；生成平台安装包使用 `npm run tauri build`。

## 当前边界

- 暂不提供多模态输入输出或跨 provider 自动 fallback。
- 不承诺远程沙箱或容器级隔离；当前边界由权限规则、命令分析和人工审批组成。
- macOS 桌面包尚未签名或 notarize，发布说明必须明确展示该限制。
- 插件来源未建立签名信任链；只应注册可信 marketplace，并优先固定 GitHub revision。

## License

[MIT](LICENSE)
