import { describe, expect, it } from "vitest";
import { buildTodayTokenMetrics, buildTokenUsageDashboard } from "@/tokenUsageDashboard";

describe("buildTokenUsageDashboard", () => {
  it("returns empty states for each dashboard scope when provider has not reported usage", () => {
    const dashboard = buildTokenUsageDashboard({});

    expect(dashboard.map((item) => item.empty)).toEqual([true, true, true]);
    expect(dashboard.map((item) => item.label)).toEqual(["Today", "Session", "Turn"]);
  });

  it("formats today, session, and turn usage with input and output breakdowns", () => {
    const dashboard = buildTokenUsageDashboard({
      todayUsage: { inputTokens: 1200, outputTokens: 345, totalTokens: 1545 },
      sessionUsage: { inputTokens: 900, outputTokens: 100, totalTokens: 1000 },
      turnUsage: { inputTokens: 200, outputTokens: 50, totalTokens: 250, reasoningTokens: 25 },
    });

    expect(dashboard).toEqual([
      {
        id: "today",
        label: "Today",
        value: "1,545",
        empty: false,
        details: ["Input 1,200", "Output 345", "Cost ¥0.0019"],
      },
      {
        id: "session",
        label: "Session",
        value: "1,000",
        empty: false,
        details: ["Input 900", "Output 100", "Cost ¥0.0011"],
      },
      {
        id: "turn",
        label: "Turn",
        value: "250",
        empty: false,
        details: ["Input 200", "Output 50", "Reasoning 25", "Cost ¥0.0003"],
      },
    ]);
  });
});

describe("buildTodayTokenMetrics", () => {
  it("returns fallback items when no usage", () => {
    const metrics = buildTodayTokenMetrics(undefined);
    expect(metrics.map((m) => m.id)).toEqual(["total", "cost"]);
    expect(metrics.map((m) => m.value)).toEqual(["-", "-"]);
  });

  it("returns total and cost with cache breakdown", () => {
    const metrics = buildTodayTokenMetrics({
      inputTokens: 1000000,
      outputTokens: 500000,
      totalTokens: 1500000,
      promptCacheHitTokens: 800000,
      promptCacheMissTokens: 200000,
    });

    expect(metrics).toEqual([
      { id: "total", label: "Total", value: "1,500,000" },
      { id: "cache", label: "Cache", value: "80.0%" },
      { id: "cost", label: "Cost", value: "¥1.22" },
    ]);
  });
});