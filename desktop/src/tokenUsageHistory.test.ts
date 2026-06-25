import { describe, expect, it } from "vitest";
import {
  addUsageToHistory,
  dateKey,
  parseTokenUsageHistory,
} from "@/tokenUsageHistory";

describe("token usage history", () => {
  it("uses a local calendar date key", () => {
    expect(dateKey(new Date(2026, 5, 20, 23, 59))).toBe("2026-06-20");
  });

  it("adds provider usage deltas into a persisted daily bucket", () => {
    const history = addUsageToHistory(
      {},
      { inputTokens: 10, outputTokens: 4, totalTokens: 14, reasoningTokens: 2 },
      "2026-06-20",
    );
    const updated = addUsageToHistory(
      history,
      { inputTokens: 6, outputTokens: 3, totalTokens: 9 },
      "2026-06-20",
    );

    expect(updated["2026-06-20"]).toEqual({
      inputTokens: 16,
      outputTokens: 7,
      totalTokens: 23,
      reasoningTokens: 2,
    });
  });

  it("ignores invalid persisted payloads", () => {
    expect(parseTokenUsageHistory("{bad json")).toEqual({});
    expect(
      parseTokenUsageHistory(JSON.stringify({ bad: { inputTokens: "x" } })),
    ).toEqual({});
  });
});
