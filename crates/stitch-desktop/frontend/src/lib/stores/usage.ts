import { writable, derived, get } from "svelte/store";
import type { LayerStats } from "$lib/types";

/** Live + last-turn token / context usage for the chat chrome. */
export type UsageState = {
  inputTokens: number;
  outputTokens: number;
  contextTokens: number;
  contextLimit: number;
  /** ReAct loop count for the in-flight or last finished turn. */
  iterations: number;
  compacted: boolean;
  /** Server-reported cache-hit input tokens (0 when absent). */
  cacheHitTokens: number;
  /** Server-reported cache-miss input tokens. */
  cacheMissTokens: number;
  /** Estimated turn cost in CNY (computed in Rust; 0 when absent). */
  cost: number;
  /** Cumulative tokens across turns in this app session (not persisted). */
  sessionTotal: number;
  /** How many agent turns finished this app session. */
  turnCount: number;
  /** Three-tier context layer breakdown (null when layering disabled). */
  layers: LayerStats | null;
};

const empty: UsageState = {
  inputTokens: 0,
  outputTokens: 0,
  contextTokens: 0,
  contextLimit: 64_000,
  iterations: 0,
  compacted: false,
  cacheHitTokens: 0,
  cacheMissTokens: 0,
  cost: 0,
  sessionTotal: 0,
  turnCount: 0,
  layers: null,
};

export const usage = writable<UsageState>({ ...empty });

export const contextPct = derived(usage, ($u) => {
  if (!$u.contextLimit) return 0;
  return Math.min(100, Math.round(($u.contextTokens * 100) / $u.contextLimit));
});

export function applyUsageEvent(ev: {
  input_tokens?: number;
  output_tokens?: number;
  context_tokens?: number;
  context_limit?: number;
  iteration?: number;
  compacted?: boolean;
  cache_hit_tokens?: number;
  cache_miss_tokens?: number;
  layers?: LayerStats | null;
}) {
  usage.update((u) => ({
    ...u,
    inputTokens: ev.input_tokens ?? u.inputTokens,
    outputTokens: ev.output_tokens ?? u.outputTokens,
    contextTokens: ev.context_tokens ?? u.contextTokens,
    contextLimit: ev.context_limit || u.contextLimit || 64_000,
    iterations: ev.iteration ?? u.iterations,
    compacted: !!ev.compacted,
    cacheHitTokens: ev.cache_hit_tokens ?? u.cacheHitTokens,
    cacheMissTokens: ev.cache_miss_tokens ?? u.cacheMissTokens,
    layers: ev.layers !== undefined ? ev.layers : u.layers,
  }));
}

export function applyDoneUsage(ev: {
  input_tokens?: number;
  output_tokens?: number;
  context_tokens?: number;
  context_limit?: number;
  iterations?: number;
  cache_hit_tokens?: number;
  cache_miss_tokens?: number;
  cost?: number;
}) {
  const inTok = ev.input_tokens ?? 0;
  const outTok = ev.output_tokens ?? 0;
  const turn = inTok + outTok;
  usage.update((u) => ({
    inputTokens: inTok,
    outputTokens: outTok,
    contextTokens: ev.context_tokens ?? u.contextTokens,
    contextLimit: ev.context_limit || u.contextLimit || 64_000,
    iterations: ev.iterations ?? u.iterations,
    compacted: u.compacted,
    cacheHitTokens: ev.cache_hit_tokens ?? u.cacheHitTokens,
    cacheMissTokens: ev.cache_miss_tokens ?? u.cacheMissTokens,
    cost: ev.cost ?? u.cost,
    sessionTotal: u.sessionTotal + turn,
    turnCount: u.turnCount + 1,
    layers: u.layers,
  }));
}

export function resetTurnUsage() {
  usage.update((u) => ({
    ...u,
    inputTokens: 0,
    outputTokens: 0,
    iterations: 0,
    compacted: false,
    layers: null,
  }));
}

export function formatTokenCount(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "0";
  if (n < 1000) return String(Math.round(n));
  if (n < 10_000) return `${(n / 1000).toFixed(1)}k`;
  return `${Math.round(n / 1000)}k`;
}

export function usageHint(): string {
  const u = get(usage);
  const hitTotal = u.cacheHitTokens + u.cacheMissTokens;
  const hitPct = hitTotal > 0 ? `${Math.round((u.cacheHitTokens * 100) / hitTotal)}%` : "—";
  const costText = u.cost > 0 ? ` · 成本 ¥${u.cost.toFixed(4)}` : "";
  return `Context ${formatTokenCount(u.contextTokens)}/${formatTokenCount(u.contextLimit)} · 缓存命中 ${hitPct}${costText}`;
}
