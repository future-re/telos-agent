// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from "vitest";
import { loadAppearance } from "@/appearance";

describe("appearance persistence", () => {
  beforeEach(() => window.localStorage.clear());

  it("falls back when a removed bundled font is stored", () => {
    window.localStorage.setItem(
      "telos.desktop.appearance",
      JSON.stringify({ font: "wenkai", theme: "warm" }),
    );

    expect(loadAppearance()).toEqual({ font: "noto-sans", theme: "warm" });
  });
});
