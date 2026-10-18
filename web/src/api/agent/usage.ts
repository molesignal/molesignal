import { http } from '@/lib/http';

/** What a model call was made for; budgets tell people-facing calls from background ones. */
export type UsageOrigin = 'chat' | 'rca' | 'inspection' | 'llm_eval';

export const USAGE_ORIGINS: readonly UsageOrigin[] = ['chat', 'rca', 'inspection', 'llm_eval'];

export type BudgetLevel = 'unlimited' | 'ok' | 'warning' | 'exhausted';

/** What happens to chats and on-demand RCA once the day's budget is used up. */
export type InteractiveBudgetMode = 'block' | 'warn';

export interface BudgetStatus {
  level: BudgetLevel;
  used_tokens: number;
  /** 0 means no limit. */
  limit_tokens: number;
  warning_percent: number;
  interactive_mode: InteractiveBudgetMode;
  /** The next UTC midnight, when the count starts again. */
  resets_at: number;
}

export interface BudgetSettings {
  /** The organization's own daily limit; null follows the instance default, 0 is unlimited. */
  org_limit: number | null;
  /** `[agent] daily_token_budget` of the instance; 0 is unlimited. */
  instance_default: number;
  effective_limit: number;
  interactive_mode: InteractiveBudgetMode;
  warning_percent: number;
  updated_by?: string | null;
  updated_at?: number | null;
}

export interface DailyUsageRow {
  day_start_micros: number;
  origin: UsageOrigin;
  calls: number;
  /** Calls whose model provider had no prices; they are not in `cost`. */
  unpriced_calls: number;
  prompt_tokens: number;
  completion_tokens: number;
  /** In `currency`, the one the model providers of these calls are priced in. */
  cost: number;
  /** ISO 4217; null on the row of calls without prices. */
  currency: string | null;
}

/** A cost with its currency (ISO 4217): the one its model provider is priced in. */
export interface Cost {
  amount: number;
  currency: string;
}

export interface AgentUsage {
  status: BudgetStatus;
  budget: BudgetSettings;
  /** Some model provider has unit prices, so costs mean something. */
  cost_known: boolean;
  days: DailyUsageRow[];
}

export interface BudgetInput {
  daily_token_limit: number | null;
  interactive_mode: InteractiveBudgetMode;
}

export async function getUsage(days = 30): Promise<AgentUsage> {
  const { data } = await http.get<AgentUsage>('/agent/usage', { params: { days } });
  return data;
}

export async function putBudget(input: BudgetInput): Promise<AgentUsage> {
  const { data } = await http.put<AgentUsage>('/agent/usage/budget', input);
  return data;
}
