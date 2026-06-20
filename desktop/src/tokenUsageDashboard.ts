import { TokenUsage } from "@/chatState";
import { formatTokenCount } from "@/tokenUsage";

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
}: {
  todayUsage?: TokenUsage;
  sessionUsage?: TokenUsage;
  turnUsage?: TokenUsage;
}): TokenUsageDashboardItem[] {
  return [
    toDashboardItem("today", "今日消耗", todayUsage),
    toDashboardItem("session", "当前会话", sessionUsage),
    toDashboardItem("turn", "当前单轮", turnUsage),
  ];
}

export function buildTodayTokenMetrics(usage?: TokenUsage): TokenUsageDashboardItem[] {
  if (!usage) {
    return [
      emptyMetric("total", "总计"),
      emptyMetric("input", "输入"),
      emptyMetric("output", "输出"),
    ];
  }

  return [
    tokenMetric("total", "总计", usage.totalTokens),
    tokenMetric("input", "输入", usage.inputTokens),
    tokenMetric("output", "输出", usage.outputTokens),
  ];
}

function toDashboardItem(
  id: TokenUsageDashboardItem["id"],
  label: string,
  usage?: TokenUsage,
): TokenUsageDashboardItem {
  if (!usage) {
    return {
      id,
      label,
      value: "暂无上报",
      empty: true,
      details: [],
    };
  }

  const details = [
    `输入 ${formatTokenCount(usage.inputTokens)}`,
    `输出 ${formatTokenCount(usage.outputTokens)}`,
  ];

  if (usage.reasoningTokens !== undefined) {
    details.push(`思考 ${formatTokenCount(usage.reasoningTokens)}`);
  }

  return {
    id,
    label,
    value: formatTokenCount(usage.totalTokens),
    empty: false,
    details,
  };
}

function emptyMetric(
  id: TokenUsageDashboardItem["id"],
  label: string,
): TokenUsageDashboardItem {
  return {
    id,
    label,
    value: "暂无",
    empty: true,
    details: [],
  };
}

function tokenMetric(
  id: TokenUsageDashboardItem["id"],
  label: string,
  tokens: number,
): TokenUsageDashboardItem {
  return {
    id,
    label,
    value: formatTokenCount(tokens),
    empty: false,
    details: [],
  };
}
