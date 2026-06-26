import { useMemo, useState } from "react";
import { TelosEvent } from "@/chatState";
import {
  TokenUsageHistory,
  addUsageToHistory,
  dateKey,
  loadTokenUsageHistory,
  saveTokenUsageHistory,
} from "@/tokenUsageHistory";

export function useTokenUsageHistory() {
  const [usageHistory, setUsageHistory] = useState<TokenUsageHistory>(() =>
    typeof window === "undefined" ? {} : loadTokenUsageHistory(),
  );
  const todayUsage = useMemo(() => usageHistory[dateKey()], [usageHistory]);

  function recordUsageEvent(event: TelosEvent) {
    const usage = usageFromEvent(event);
    if (!usage) {
      return;
    }

    setUsageHistory((current) => {
      const next = addUsageToHistory(current, usage);
      saveTokenUsageHistory(next);
      return next;
    });
  }

  return {
    recordUsageEvent,
    todayUsage,
    usageHistory,
  };
}

function usageFromEvent(event: TelosEvent) {
  if (event.inputTokens === undefined || event.outputTokens === undefined) {
    return undefined;
  }

  return {
    inputTokens: event.inputTokens,
    outputTokens: event.outputTokens,
    totalTokens: event.totalTokens ?? event.inputTokens + event.outputTokens,
    promptCacheHitTokens: event.promptCacheHitTokens,
    promptCacheMissTokens: event.promptCacheMissTokens,
    reasoningTokens: event.reasoningTokens,
  };
}
