import { describe, expect, it } from "vitest";
import { editableConfig } from "@/components/PluginSettings";

describe("editableConfig", () => {
  it("keeps ordinary values in the editor while excluding redacted secrets", () => {
    expect(
      editableConfig({
        id: "configured@test",
        name: "configured",
        status: "enabled",
        errors: [],
        configSchema: {
          token: { type: "string", sensitive: true },
          mode: { type: "string" },
        },
        config: { token: "[REDACTED]", mode: "strict" },
      }),
    ).toEqual({ mode: "strict" });
  });
});
