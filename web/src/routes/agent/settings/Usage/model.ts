import type {
  BudgetInput,
  BudgetStatus,
  DailyUsageRow,
  InteractiveBudgetMode,
  UsageOrigin,
} from '@/api/agent/usage';
import { USAGE_ORIGINS } from '@/api/agent/usage';

import type { Costs } from '../money';

const MICROS_PER_DAY = 86_400_000_000;

const TOKENS = new Intl.NumberFormat();

/** A token count in full: budgets are read to the token. */
export function formatTokens(value: number): string {
  return TOKENS.format(Math.max(0, Math.round(value)));
}

/** How much of the day's limit is used, in percent; null when there is no limit. */
export function usedPercent(status: Pick<BudgetStatus, 'used_tokens' | 'limit_tokens'>): number | null {
  if (status.limit_tokens <= 0) return null;
  return Math.round((status.used_tokens * 100) / status.limit_tokens);
}

/** The start of the UTC day a status belongs to: one day before it resets. */
export function statusDayStart(status: Pick<BudgetStatus, 'resets_at'>): number {
  return status.resets_at - MICROS_PER_DAY;
}

export interface OriginUsage {
  calls: number;
  /** Calls whose model provider had no prices: `costs` leaves them out. */
  unpricedCalls: number;
  tokens: number;
  /** In the currency of each model provider, never added across currencies. */
  costs: Costs;
}

function emptyOrigin(): OriginUsage {
  return { calls: 0, unpricedCalls: 0, tokens: 0, costs: {} };
}

function emptyByOrigin(): Record<UsageOrigin, OriginUsage> {
  return Object.fromEntries(USAGE_ORIGINS.map((origin) => [origin, emptyOrigin()])) as Record<
    UsageOrigin,
    OriginUsage
  >;
}

export interface DayUsage extends OriginUsage {
  dayStartMicros: number;
  byOrigin: Record<UsageOrigin, OriginUsage>;
}

/** One row per day that had calls, newest first, each split by origin. */
export function usageByDay(rows: readonly DailyUsageRow[]): DayUsage[] {
  const days = new Map<number, DayUsage>();
  for (const row of rows) {
    const day = days.get(row.day_start_micros) ?? {
      ...emptyOrigin(),
      dayStartMicros: row.day_start_micros,
      byOrigin: emptyByOrigin(),
    };
    const origin = day.byOrigin[row.origin] ?? emptyOrigin();
    for (const total of [origin, day]) {
      total.calls += row.calls;
      total.unpricedCalls += row.unpriced_calls;
      total.tokens += row.prompt_tokens + row.completion_tokens;
      if (row.currency) total.costs[row.currency] = (total.costs[row.currency] ?? 0) + row.cost;
    }
    day.byOrigin[row.origin] = origin;
    days.set(row.day_start_micros, day);
  }
  return [...days.values()].sort((a, b) => b.dayStartMicros - a.dayStartMicros);
}

/** How the organization's own limit is set. */
export type LimitChoice = 'inherit' | 'custom' | 'unlimited';

export function limitChoice(orgLimit: number | null | undefined): LimitChoice {
  if (orgLimit == null) return 'inherit';
  return orgLimit === 0 ? 'unlimited' : 'custom';
}

/** What to save for the form; null when a custom limit is not a positive whole number. */
export function budgetInput(
  choice: LimitChoice,
  customLimit: string,
  interactiveMode: InteractiveBudgetMode,
): BudgetInput | null {
  if (choice === 'inherit') return { daily_token_limit: null, interactive_mode: interactiveMode };
  if (choice === 'unlimited') return { daily_token_limit: 0, interactive_mode: interactiveMode };
  const limit = Number(customLimit.trim());
  if (!customLimit.trim() || !Number.isInteger(limit) || limit <= 0) return null;
  return { daily_token_limit: limit, interactive_mode: interactiveMode };
}
