# README 全面重写设计文档

## 目标

将 `README.md` 从当前过于简化的版本更新为一份准确、完整、结构清晰的项目入口文档，真实反映代码库已实现的功能、设计与架构。

## 当前问题

1. **大量已实现能力未文档化**：重试/退避、取消、权限引擎、人机审批、bash 安全分析、metrics、prompt 装配、skills、memory、storage、compaction、streaming、thinking block 等在 README 中均未提及。
2. **示例代码无法编译**：`AgentConfig` 字段已从 `system_prompt` 改为 `base_system_prompt`；`CompletionResponse` 缺少 `usage` 字段。
3. **"暂不包含" 列表过时**：thinking block 已实现；权限审批流程已部分实现；单 provider 重试已实现。
4. **架构与执行流程描述过简**：未体现 prompt 构建、compaction、重试、hook、权限/审批、工具执行流水线、持久化/回滚等关键阶段。

## 用户选择

- 修改范围：功能、设计、架构三部分全面更新。
- 方案：方案 A（全面重写），已获确认。

## 新版 README 结构

1. **项目简介** — 一句话定位。
2. **定位与使用场景** — 实验底座、运行时内核。
3. **功能特性** — 按子系统分组：
   - 核心运行时
   - Provider 适配
   - 工具系统
   - 权限与审批
   - Prompt 与 Skills
   - Memory
   - 存储与恢复
   - Compaction 与预算
   - Streaming 与事件
4. **架构概览** — 分层文字描述（session / runtime / provider / tool / storage / prompt / skills / memory / permissions）。
5. **执行流程** — 8–10 步详细流程，覆盖 prompt 构建、compaction、provider 调用、hook、tool 验证/权限/审批/执行、结果回注、stop hook、持久化与错误回滚。
6. **核心对象** — 表格列出主要类型/traits 及职责。
7. **最小示例** — 修正后可编译的 Rust 代码。
8. **运行示例** — `kimi_tool_loop` 示例。
9. **测试** — `cargo test`。
10. **暂不包含** — 更新后的边界说明。

## 关键内容要点

### 功能特性新增条目

- Provider 重试退避（`RetryConfig`）
- 取消检查点（`AgentConfig::cancelled`）
- 人机审批（`ApprovalHandler` / `ApprovalRequest` / `ApprovalDecision`）
- 规则权限引擎（`PermissionEngine` / `PermissionRule` / `RuleDecision`）
- Bash AST 安全分析（`bash_security`）
- 会话指标（`SessionMetrics`）
- Prompt 装配与静态缓存（`PromptAssembly` / `PromptSection` / builtins）
- Skills 系统（`Skill` / `SkillRegistry` / `SkillLoader` / `SkillTool`）
- Memory 系统（`MemoryStore` 及 5 个 memory 工具）
- Tool 别名与 JSON Schema 校验
- 工具超时（`tool_timeout_ms`）
- 文件读取上限（`max_file_read_bytes`）
- 文件写冲突保护（`FileReadState` / `FileReadRecord`）
- `NoopStorage`、`ErasedProvider`
- Thinking / reasoning blocks
- 流式工具执行 API（`execute_tool_calls_stream`）

### 示例代码修正

- `system_prompt: Some(...)` → `base_system_prompt: Some(...)`
- `CompletionResponse { message, stop_reason }` → 补齐 `usage: None`

### "暂不包含" 更新

- 移除：thinking block（已实现）
- 调整表述：权限审批流程已部分实现（无 sandbox）；provider 重试已部分实现（单 provider 内，无跨 provider fallback）
- 保留：UI/TUI/Web、MCP/plugin/bridge/swarm、多模态、跨 provider fallback

## 验收标准

- [ ] README 中所有新增条目与代码库实际能力一致，不夸大。
- [ ] 最小示例代码可在当前代码库编译通过。
- [ ] "暂不包含" 列表不再包含已实现内容。
- [ ] 执行流程描述覆盖审计报告中的主要阶段。
- [ ] 核心对象表格包含 README 当前遗漏的重要类型/traits。
