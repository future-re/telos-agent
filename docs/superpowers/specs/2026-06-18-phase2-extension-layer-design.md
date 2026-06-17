# Phase 2: Extension Layer — Design Spec

**Date:** 2026-06-18
**Status:** Design approved
**Scope:** MCP Client Integration + Additional Tools
**Dependencies:** Phase 1 (Prompt System refactor required for MCP tool injection)

---

## Architecture Overview

```
┌──────────────────────────────────────────────────────────┐
│                     AgentSession                          │
│  ┌──────────────────────────────────────────────────┐    │
│  │                 ToolRegistry                       │    │
│  │  ┌──────────┐  ┌──────────┐  ┌────────────────┐  │    │
│  │  │  Built-in │  │  Skill   │  │  MCP Tools     │  │    │
│  │  │  Tools   │  │  Tool    │  │  (bridged)     │  │    │
│  │  └──────────┘  └──────────┘  └───────┬────────┘  │    │
│  └──────────────────────────────────────┼────────────┘    │
│                                         │                  │
│  ┌──────────────────────────────────────┼────────────┐    │
│  │            MCP Manager (pulseengine/mcp)          │    │
│  │  ┌────────┐  ┌────────┐  ┌────────┐  │            │    │
│  │  │Server A│  │Server B│  │Server C│  │            │    │
│  │  │(stdio) │  │ (stdio) │  │ (stdio) │  │            │    │
│  │  └────────┘  └────────┘  └────────┘  │            │    │
│  └──────────────────────────────────────┼────────────┘    │
└──────────────────────────────────────────────────────────┘
```

---

## 1. Dependency: pulseengine/mcp

We use `pulseengine/mcp` for MCP protocol and transport, NOT build our own.

```toml
# Cargo.toml additions
mcp-client = "0.17"
mcp-transport = "0.17"
# Note: mcp-protocol is transitive, not direct
```

**What the library handles (we don't write):**
- JSON-RPC 2.0 message serialization/deserialization
- MCP initialize handshake (capability negotiation, client/server info)
- Stdio transport (spawn process, stdin/stdout framing)
- tools/list, tools/call, resources/list, resources/read
- Error handling and protocol-level retry

**What we build (orchestration layer):**
- `McpManager` — multi-server lifecycle management
- Config loading from `.mcp.json` / `settings.json`
- `McpToolBridge` — MCP tool → internal `Tool` trait adaptation
- Per-server permission gating
- Reconnect with exponential backoff

---

## 2. Module Structure

```
src/mcp/
  mod.rs            — module exports + McpManager
  config.rs         — .mcp.json / settings.json mcpServers loading
  bridge.rs         — McpToolBridge (MCP tool → Tool trait)
  permissions.rs    — per-server permission model
  retry.rs          — reconnect with exponential backoff (wraps library)
```

**Estimated total:** ~400 lines of new code (library does the heavy lifting).

---

## 3. Core Types

### 3.1 McpManager

```rust
/// Manages all MCP server connections
struct McpManager {
    servers: HashMap<String, McpServerHandle>,
    config_path: PathBuf,       // .tiny-agent/mcp.json
}

struct McpServerHandle {
    id: String,                      // unique server ID
    config: McpServerConfig,         // from config file
    client: McpClient,              // pulseengine/mcp client
    tools: Vec<McpToolDefinition>,   // cached tool list
    state: McpConnectionState,
    retry_count: u32,
}

#[derive(Clone)]
struct McpServerConfig {
    command: String,                 // e.g., "npx"
    args: Vec<String>,              // e.g., ["-y", "@anthropic/mcp-filesystem"]
    env: HashMap<String, String>,   // additional env vars
    cwd: Option<PathBuf>,           // working directory
    auto_connect: bool,             // connect on session start (default: true)
    timeout_ms: u64,                // request timeout (default: 60_000)
}

enum McpConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed { error: String, since: Instant },
}
```

### 3.2 McpToolBridge

```rust
/// Wraps an MCP server tool as a native Tool
struct McpToolBridge {
    server_id: String,
    tool_def: McpToolDefinition,
    mcp_manager: Arc<McpManager>,
}

impl Tool for McpToolBridge {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            // Name normalization: "mcp__<server>__<tool>"
            name: format!("mcp__{}__{}", self.server_id, self.tool_def.name),
            description: format!("[MCP:{}] {}", self.server_id, self.tool_def.description),
            input_schema: self.tool_def.input_schema.clone(),
        }
    }

    fn is_concurrency_safe(&self) -> bool { false }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> ToolOutput {
        let result = self.mcp_manager
            .call_tool(&self.server_id, &self.tool_def.name, args)
            .await?;
        ToolOutput::json(result)
    }

    fn check_permission(&self, _args: &Value) -> PermissionDecision {
        PermissionDecision::Ask {
            reason: format!("MCP tool '{}' from server '{}'", self.tool_def.name, self.server_id)
        }
    }
}
```

---

## 4. Configuration Format

Compatible with Claude Code's MCP config format:

```json
// .tiny-agent/mcp.json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/alin"],
      "auto_connect": true
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_TOKEN": "${GITHUB_TOKEN}"
      }
    },
    "brave-search": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-brave-search"],
      "env": {
        "BRAVE_API_KEY": "${BRAVE_API_KEY}"
      }
    }
  }
}
```

**Config loading order (later merges and overrides earlier):**
1. Built-in default (empty)
2. `.tiny-agent/mcp.json` (project)
3. `~/.tiny-agent/mcp.json` (user)

---

## 5. Lifecycle

```
Session Start
     │
     ▼
McpManager::load_config()          ← read .mcp.json files
     │
     ▼
McpManager::connect_all()          ← spawn all auto_connect servers
     │
     ├─ Server A: stdio connect → initialize → tools/list → cache tools
     ├─ Server B: spawn fails → state = Failed, schedule retry
     └─ Server C: connected, tools cached
     │
     ▼
ToolRegistry::register_mcp_tools() ← register McpToolBridge for each tool
     │
     ▼
PromptSection renders MCP tools    ← model sees MCP tools in system prompt
     │
     ▼
AgentSession::run_turn()
     │
     ▼ (model calls mcp__filesystem__read_file)
     │
McpToolBridge::invoke()
     │
     ▼
McpManager::call_tool("filesystem", "read_file", args)
     │
     ▼
McpServerHandle::client.call_tool()  ← pulseengine/mcp handles JSON-RPC
     │
     ▼
Session End
     │
     ▼
McpManager::disconnect_all()       ← kill subprocesses
```

---

## 6. Permission Model

```rust
// Per-server permission, evaluated on first tool call per turn
impl McpManager {
    fn check_server_permission(&self, server_id: &str) -> PermissionDecision {
        // Check if user has set a rule for this server
        // Default: Ask (first use per session)
        // Can be: Allow (always), Deny (never)
    }
}
```

- **Default: Ask** — first tool call from each server requires user approval
- **Per-server rules** — user can set "always allow filesystem"
- **Per-tool granularity** — deferred to Phase 2b (not needed for initial release)
- **Permission is cached per turn** — once allowed for a server in a turn, subsequent calls from same server are auto-allowed

---

## 7. Retry Strategy

```rust
impl McpManager {
    async fn reconnect(&mut self, server_id: &str) -> Result<()> {
        let handle = &mut self.servers[server_id];
        let delay = std::cmp::min(
            1000 * 2u64.pow(handle.retry_count),
            60_000  // max 60 seconds
        );
        handle.retry_count += 1;
        tokio::time::sleep(Duration::from_millis(delay)).await;
        handle.client.connect().await
    }
}
```

- Max 3 retries, then state = Failed (permanent for this session)
- Failed servers don't block session startup
- User can trigger manual reconnect via tool/MCP manager UI

---

## 8. Additional Tools (Phase 2)

Three new built-in tools to extend agent capabilities:

### 8.1 WebFetchTool

```rust
struct WebFetchTool;
// Aliases: ["web_fetch"]
// Fetches URL, converts HTML to markdown, returns result
// - HTTP upgraded to HTTPS
// - Cross-host redirects returned to model (not auto-followed)
// - Response cached for 15 minutes per URL
// - Max response size: configurable, default 256KB
```

**Dependency:** `reqwest` (already present transitively? check) + basic HTML-to-text

### 8.2 WebSearchTool

```rust
struct WebSearchTool;
// Aliases: ["web_search"]
// Performs web search, returns titles + URLs + snippets
// - Configurable search provider (default: DuckDuckGo, no API key needed)
// - Optional: Brave Search API for better results
// - Returns top 10 results
```

**Dependency:** `ddg` crate or simple HTTP call to DuckDuckGo API (no key needed)

### 8.3 AskUserQuestionTool

```rust
struct AskUserQuestionTool;
// Aliases: ["ask_user"]
// Asks the user one or more questions interactively
// - Supports single-select and multi-select
// - Each question: header + options (2-4) + descriptions
// - Returns user's answers
```

**No new dependency** — uses stdin/stdout or terminal interaction.

---

## 9. Dependencies Added

```toml
mcp-client = "0.17"
mcp-transport = "0.17"
# Note: mcp-protocol is transitive

# For WebFetchTool
reqwest = { version = "0.12", features = ["json", "gzip"] }  # may already exist
# HTML to text: use a minimal implementation (< 100 lines, no external dep)

# For WebSearchTool
# Option A: DuckDuckGo (free, no API key) — simple HTTP GET, no dep
# Option B: Brave Search (better but needs key, already covered by MCP brave-search)
```

---

## 10. Testing Strategy

### MCP
- Unit test: config loading from `.mcp.json` (valid, invalid, empty)
- Unit test: McpToolBridge wraps tool definitions correctly
- Unit test: permission gating per server
- Integration test: spawn a simple echo MCP server (can be a shell script), verify connect + list_tools + call_tool
- Integration test: server disconnect/reconnect cycle
- Integration test: failed server doesn't block session startup

### WebFetch/WebSearch
- Unit test: URL normalization (HTTP → HTTPS)
- Integration test: fetch a known static page
- Integration test: search returns results

### AskUserQuestion
- Difficult to test interactively — test schema parsing and answer validation
- Integration test: simulate with a mock input

---

## 11. File Layout After Phase 2

```
tiny-agent-core/
├── src/
│   ├── mcp/                        # NEW
│   │   ├── mod.rs
│   │   ├── config.rs
│   │   ├── bridge.rs
│   │   ├── permissions.rs
│   │   └── retry.rs
│   ├── skills/                     # Phase 1
│   ├── prompt/                     # Phase 1
│   ├── memory/                     # Phase 1
│   ├── tool/
│   │   ├── mod.rs                  # MODIFIED: register Skill/Memory/MCP tools
│   │   └── validate.rs
│   ├── tools/
│   │   ├── mod.rs                  # MODIFIED: register_core_tools adds new tools
│   │   ├── web_fetch.rs            # NEW
│   │   ├── web_search.rs           # NEW
│   │   ├── ask_user_question.rs    # NEW
│   │   └── ... (existing tools)
│   ├── config.rs                   # MODIFIED: add MCP config path
│   ├── runtime.rs                  # MODIFIED: MCP lifecycle hooks
│   └── ...
├── Cargo.toml                      # MODIFIED: mcp-client, mcp-transport, reqwest
└── docs/superpowers/specs/
    ├── 2026-06-18-phase1-core-intelligence-design.md
    └── 2026-06-18-phase2-extension-layer-design.md
```
