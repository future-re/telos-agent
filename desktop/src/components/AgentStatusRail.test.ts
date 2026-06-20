import { describe, expect, it } from "vitest";
import { deriveRuntimeSubagents } from "@/components/AgentStatusRail";
import { ToolActivity } from "@/chatState";

describe("deriveRuntimeSubagents", () => {
  it("returns only running subagent tools", () => {
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
    ]);
  });
});
