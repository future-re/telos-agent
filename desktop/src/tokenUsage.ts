import { TokenUsage } from "@/chatState";

export function sumTokenUsage(usages: Array<TokenUsage | undefined>): TokenUsage | undefined {
  const defined = usages.filter(Boolean) as TokenUsage[];
  if (defined.length === 0) {
    return undefined;
  }

  return defined.reduce<TokenUsage>(
    (total, usage) => ({
      inputTokens: total.inputTokens + usage.inputTokens,
      outputTokens: total.outputTokens + usage.outputTokens,
      totalTokens: total.totalTokens + usage.totalTokens,
      promptCacheHitTokens: addOptional(total.promptCacheHitTokens, usage.promptCacheHitTokens),
      promptCacheMissTokens: addOptional(total.promptCacheMissTokens, usage.promptCacheMissTokens),
      reasoningTokens: addOptional(total.reasoningTokens, usage.reasoningTokens),
    }),
    { inputTokens: 0, outputTokens: 0, totalTokens: 0 },
  );
}

export function formatTokenCount(tokens: number): string {
  return new Intl.NumberFormat("zh-CN").format(tokens);
}

function addOptional(current: number | undefined, next: number | undefined): number | undefined {
  if (current === undefined && next === undefined) {
    return undefined;
  }
  return (current ?? 0) + (next ?? 0);
}
