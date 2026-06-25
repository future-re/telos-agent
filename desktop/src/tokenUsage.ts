import { TokenUsage } from "@/chatState";

export function sumTokenUsage(
  usages: Array<TokenUsage | undefined>,
): TokenUsage | undefined {
  const defined = usages.filter(Boolean) as TokenUsage[];
  if (defined.length === 0) {
    return undefined;
  }

  return defined.reduce<TokenUsage>(
    (total, usage) => ({
      inputTokens: total.inputTokens + usage.inputTokens,
      outputTokens: total.outputTokens + usage.outputTokens,
      totalTokens: total.totalTokens + usage.totalTokens,
      promptCacheHitTokens: addOptional(
        total.promptCacheHitTokens,
        usage.promptCacheHitTokens,
      ),
      promptCacheMissTokens: addOptional(
        total.promptCacheMissTokens,
        usage.promptCacheMissTokens,
      ),
      reasoningTokens: addOptional(
        total.reasoningTokens,
        usage.reasoningTokens,
      ),
    }),
    { inputTokens: 0, outputTokens: 0, totalTokens: 0 },
  );
}

export function formatTokenCount(tokens: number): string {
  return new Intl.NumberFormat("zh-CN").format(tokens);
}

// ── Cost estimation ──────────────────────────────────────────────

export interface CostEstimate {
  inputCacheHit: number;
  inputCacheMiss: number;
  outputCost: number;
  totalCost: number;
}

// DeepSeek V4 pricing (yuan per million tokens).
const MODEL_PRICING: Record<
  string,
  { hit: number; miss: number; output: number }
> = {
  "deepseek-v4-flash": {
    hit: 0.02,
    miss: 1.0,
    output: 2.0,
  },
  "deepseek-v4-pro": {
    hit: 0.0261,
    miss: 3.132,
    output: 6.264,
  },
};

/**
 * Estimate cost for a single usage report.
 * Falls back to cache-miss pricing when breakdown is unavailable.
 * Defaults to Flash pricing when model is unknown (e.g. today's aggregated usage).
 */
export function estimateCost(
  model: string | undefined | null,
  usage: TokenUsage,
): CostEstimate | undefined {
  const resolvedModel = model ?? "deepseek-v4-flash";
  const pricing =
    MODEL_PRICING[resolvedModel.toLowerCase()] ??
    MODEL_PRICING["deepseek-v4-flash"];

  const [hitTokens, missTokens] = resolveCacheBreakdown(usage);

  const inputCacheHit = (hitTokens * pricing.hit) / 1_000_000;
  const inputCacheMiss = (missTokens * pricing.miss) / 1_000_000;
  const outputCost = (usage.outputTokens * pricing.output) / 1_000_000;

  return {
    inputCacheHit,
    inputCacheMiss,
    outputCost,
    totalCost: inputCacheHit + inputCacheMiss + outputCost,
  };
}

function resolveCacheBreakdown(usage: TokenUsage): [number, number] {
  const {
    promptCacheHitTokens: hit,
    promptCacheMissTokens: miss,
    inputTokens,
  } = usage;
  if (hit !== undefined && miss !== undefined) return [hit, miss];
  if (hit !== undefined) return [hit, Math.max(0, inputTokens - hit)];
  if (miss !== undefined) return [Math.max(0, inputTokens - miss), miss];
  return [0, inputTokens];
}

/** Format cost in yuan, matching the TUI billing.rs style. */
export function formatCost(cost: number): string {
  if (cost >= 1_000_000) return `¥${(cost / 1_000_000).toFixed(1)}m`;
  if (cost >= 1_000) return `¥${(cost / 1_000).toFixed(1)}k`;
  if (cost >= 1.0) return `¥${cost.toFixed(2)}`;
  if (cost >= 0.01) return `¥${cost.toFixed(3)}`;
  if (cost > 0.0) return `¥${cost.toFixed(4)}`;
  return "¥0";
}

function addOptional(
  current: number | undefined,
  next: number | undefined,
): number | undefined {
  if (current === undefined && next === undefined) {
    return undefined;
  }
  return (current ?? 0) + (next ?? 0);
}
