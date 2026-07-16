# telos_agent_host

Shared application-host support for Telos. This crate sits above `telos_agent`
and provides configuration loading, project discovery, provider construction,
memory initialization, and runtime/tool assembly for the CLI and desktop app.

It intentionally contains no agent-loop implementation; hosts drive turns
through `telos_agent::AgentRuntime` and `telos_agent::AgentSession`.
