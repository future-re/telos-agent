import { TokenUsage } from "@/chatState";
import { estimateCost, formatCost, formatTokenCount } from "@/tokenUsage";

export interface TokenUsageDashboardItem {
  id: "today" | "session" | "turn" | "total" | "input" | "output";
  label: string;
  value: string;
  empty: boolean;
  details: string[];
}

export function buildTokenUsageDashboard({
  sessionUsage,
  todayUsage,
  turnUsage,
  turnModel,
}: {
  todayUsage?: TokenUsage;
  sessionUsage?: TokenUsage;
  turnUsage?: TokenUsage;
  turnModel?: string | null;
}): TokenUsageDashboardItem[] {
  return [
    toDashboardItem("today", "Today", todayUsage),
    toDashboardItem("session", "Session", sessionUsage),
    toDashboardItem("turn", "Turn", turnUsage, turnModel),
  ];
}

export interface TodayMetric {
  id: string;
  label: string;
  value: string;
}

export function buildTodayTokenMetrics(usage?: TokenUsage): TodayMetric[] {
  if (!usage) {
    return [
      { id: "total", label: "Total", value: "-" },
      { id: "cost", label: "Cost", value: "-" },
    ];
  }

  const items: TodayMetric[] = [
    { id: "total", label: "Total", value: formatTokenCount(usage.totalTokens) },
  ];

  if (
    usage.promptCacheHitTokens !== undefined &&
    usage.promptCacheMissTokens !== undefined
  ) {
    const total = usage.promptCacheHitTokens + usage.promptCacheMissTokens;
    const rate =
      total > 0
        ? ((usage.promptCacheHitTokens / total) * 100).toFixed(1)
        : "0.0";
    items.push({
      id: "cache",
      label: "Cache",
      value: `${rate}%`,
    });
  }

  const cost = estimateCost(usage.model ?? undefined, usage);
  if (cost && cost.totalCost > 0) {
    items.push({
      id: "cost",
      label: "Cost",
      value: formatCost(cost.totalCost),
    });
  }

  return items;
}

function toDashboardItem(
  id: TokenUsageDashboardItem["id"],
  label: string,
  usage?: TokenUsage,
  model?: string | null,
): TokenUsageDashboardItem {
  if (!usage) {
    return {
      id,
      label,
      value: "No usage yet",
      empty: true,
      details: [],
    };
  }

  const details = [
    `Input ${formatTokenCount(usage.inputTokens)}`,
    `Output ${formatTokenCount(usage.outputTokens)}`,
  ];

  if (usage.reasoningTokens !== undefined) {
    details.push(`Reasoning ${formatTokenCount(usage.reasoningTokens)}`);
  }

  if (
    usage.promptCacheHitTokens !== undefined &&
    usage.promptCacheMissTokens !== undefined
  ) {
    const total = usage.promptCacheHitTokens + usage.promptCacheMissTokens;
    const rate =
      total > 0
        ? ((usage.promptCacheHitTokens / total) * 100).toFixed(1)
        : "0.0";
    details.push(`Cache ${rate}%`);
  }

  if (usage.promptCacheHitTokens !== undefined) {
    details.push(`Cache hit ${formatTokenCount(usage.promptCacheHitTokens)}`);
  }
  if (usage.promptCacheMissTokens !== undefined) {
    details.push(`Cache miss ${formatTokenCount(usage.promptCacheMissTokens)}`);
  }

  const cost = estimateCost(model ?? undefined, usage);
  if (cost && cost.totalCost > 0) {
    details.push(`Cost ${formatCost(cost.totalCost)}`);
  }

  return {
    id,
    label,
    value: formatTokenCount(usage.totalTokens),
    empty: false,
    details,
  };
}
