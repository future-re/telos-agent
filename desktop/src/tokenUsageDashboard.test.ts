import { describe, expect, it } from "vitest";
import { buildTodayTokenMetrics, buildTokenUsageDashboard } from "@/tokenUsageDashboard";

describe("buildTokenUsageDashboard", () => {
  it("returns empty states for each dashboard scope when provider has not reported usage", () => {
    const dashboard = buildTokenUsageDashboard({});

    expect(dashboard.map((item) => item.empty)).toEqual([true, true, true]);
    expect(dashboard.map((item) => item.label)).toEqual(["今日消耗", "当前会话", "当前单轮"]);
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
        label: "今日消耗",
        value: "1,545",
        empty: false,
        details: ["输入 1,200", "输出 345"],
      },
      {
        id: "session",
        label: "当前会话",
        value: "1,000",
        empty: false,
        details: ["输入 900", "输出 100"],
      },
      {
        id: "turn",
        label: "当前单轮",
        value: "250",
        empty: false,
        details: ["输入 200", "输出 50", "思考 25"],
      },
    ]);
  });

  it("builds compact horizontal metrics for today's right panel", () => {
    expect(
      buildTodayTokenMetrics({ inputTokens: 1200, outputTokens: 345, totalTokens: 1545 }),
    ).toEqual([
      { id: "total", label: "总计", value: "1,545", empty: false, details: [] },
      { id: "input", label: "输入", value: "1,200", empty: false, details: [] },
      { id: "output", label: "输出", value: "345", empty: false, details: [] },
    ]);
  });
});
