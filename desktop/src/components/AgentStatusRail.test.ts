import { describe, expect, it } from "vitest";
import { deriveRuntimeSubagents } from "@/components/AgentStatusRail";
import { ToolActivity } from "@/chatState";

describe("deriveRuntimeSubagents", () => {
  it("keeps recent subagent lifecycle tools across statuses", () => {
    const tools: ToolActivity[] = [
      {
        id: "subagent-running",
        name: "subagent",
        detail: "Inspect platform behavior",
        status: "running",
        isError: false,
      },
      {
        id: "subagent-completed",
        name: "subagent",
        detail: "Finished review",
        status: "completed",
        isError: false,
      },
      {
        id: "fork-failed",
        name: "fork",
        detail: "Failed fork lens",
        status: "failed",
        isError: true,
      },
      {
        id: "web-search",
        name: "WebSearch",
        detail: "query",
        status: "running",
        isError: false,
      },
    ];

    expect(deriveRuntimeSubagents(tools)).toEqual([
      {
        id: "subagent-running",
        name: "Inspect platform behavior",
        detail: "Inspect platform behavior",
        status: "running",
      },
      {
        id: "subagent-completed",
        name: "Finished review",
        detail: "Finished review",
        status: "completed",
      },
      {
        id: "fork-failed",
        name: "Failed fork lens",
        detail: "Failed fork lens",
        status: "failed",
      },
    ]);
  });

  it("recognizes multi-agent tool names and extracts short agent ids", () => {
    const tools: ToolActivity[] = [
      {
        id: "spawn-1",
        name: "multi_agent_v1.spawn_agent",
        detail: '{"agent_id":"019ee872-558a-7321-9290-dc06e9f4680a","nickname":"Poincare"}',
        status: "completed",
        isError: false,
      },
      {
        id: "wait-1",
        name: "multi_agent_v1.wait_agent",
        detail: '{"targets":["019ee872-558a-7321-9290-dc06e9f4680a"]}',
        status: "running",
        isError: false,
      },
      {
        id: "close-1",
        name: "multi_agent_v1.close_agent",
        detail: '{"target":"019ee872-558a-7321-9290-dc06e9f4680a"}',
        status: "completed",
        isError: false,
      },
    ];

    expect(deriveRuntimeSubagents(tools)).toEqual([
      {
        id: "spawn-1",
        name: "Spawn subagent",
        detail: "id 019ee872 · 完成",
        status: "completed",
      },
      {
        id: "wait-1",
        name: "Wait for subagent",
        detail: '{"targets":["019ee872-558a-7321-9290-dc06e9f4680a"]}',
        status: "running",
      },
      {
        id: "close-1",
        name: "Close subagent",
        detail: "id 019ee872 · 完成",
        status: "completed",
      },
    ]);
  });
});
