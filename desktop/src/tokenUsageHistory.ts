import { TokenUsage } from "@/chatState";
import { sumTokenUsage } from "@/tokenUsage";

const STORAGE_KEY = "telos.tokenUsageHistory.v1";

export type TokenUsageHistory = Record<string, TokenUsage>;

export function dateKey(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function addUsageToHistory(
  history: TokenUsageHistory,
  usage: TokenUsage,
  key = dateKey(),
): TokenUsageHistory {
  return {
    ...history,
    [key]: sumTokenUsage([history[key], usage]) ?? usage,
  };
}

export function loadTokenUsageHistory(storage: Storage = window.localStorage): TokenUsageHistory {
  return parseTokenUsageHistory(storage.getItem(STORAGE_KEY));
}

export function saveTokenUsageHistory(
  history: TokenUsageHistory,
  storage: Storage = window.localStorage,
) {
  storage.setItem(STORAGE_KEY, JSON.stringify(history));
}

export function parseTokenUsageHistory(raw: string | null): TokenUsageHistory {
  if (!raw) {
    return {};
  }

  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }

    return Object.fromEntries(
      Object.entries(parsed)
        .filter(([key, value]) => /^\d{4}-\d{2}-\d{2}$/.test(key) && isTokenUsage(value))
        .map(([key, value]) => [key, value as TokenUsage]),
    );
  } catch {
    return {};
  }
}

function isTokenUsage(value: unknown): value is TokenUsage {
  if (!value || typeof value !== "object") {
    return false;
  }
  const usage = value as Partial<TokenUsage>;
  return (
    typeof usage.inputTokens === "number" &&
    typeof usage.outputTokens === "number" &&
    typeof usage.totalTokens === "number"
  );
}
