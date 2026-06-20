import { describe, expect, it } from "vitest";
import { defaultAgent } from "@/agentModel";

describe("defaultAgent", () => {
  it("keeps the desktop client anchored to the primary runtime agent", () => {
    expect(defaultAgent).toMatchObject({
      id: "primary",
      kind: "primary",
      name: "Telos Agent",
    });
  });
});
