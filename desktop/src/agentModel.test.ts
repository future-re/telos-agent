import { describe, expect, it } from "vitest";
import { defaultAgent, forkSubagent } from "@/agentModel";

describe("forkSubagent", () => {
  it("creates a subagent fork without replacing the parent agent", () => {
    const subagent = forkSubagent(defaultAgent, {
      id: "subagent-1",
      name: "UI Polish Agent",
      role: "专注桌面 UI 细节",
      instructions: "优先处理布局、字体和交互质感。",
    });

    expect(subagent).toMatchObject({
      id: "subagent-1",
      parentId: defaultAgent.id,
      kind: "subagent",
      name: "UI Polish Agent",
      role: "专注桌面 UI 细节",
      instructions: "优先处理布局、字体和交互质感。",
    });
    expect(defaultAgent.kind).toBe("primary");
  });
});
