import { http } from '@/lib/http';

export type UsageDimension = 'credential' | 'stream' | 'service';
export type BudgetPeriod = 'day' | 'month';
export type BudgetState = 'ok' | 'warning' | 'exceeded';

/** What a source is called. `name` is its id when it no longer exists (`missing`). */
export interface SourceLabel {
  kind: 'api_token' | 'connector' | 'user' | 'internal' | 'stream' | 'service';
  /** The kind of token or connector, or the type of the stream. */
  subkind: string | null;
  name: string;
  /** A token's prefix, a person's email. */
  detail: string | null;
  missing: boolean;
}

/** A budget and how it stands in the period under way. Figures that are not known are `null`. */
export interface IntakeBudget {
  id: string;
  dimension: UsageDimension;
  sourceKey: string;
  period: BudgetPeriod;
  /** In MiB. */
  limitSize: number;
  /** The percentage to warn at; 0 warns at none. */
  warnPercent: number;
  notifyUserIds: string[];
  updatedAt: number;
  periodStart: number;
  usedBytes: number | null;
  /** False while metering does not cover the whole period: `used_bytes` is then a lower bound. */
  usedComplete: boolean;
  usedRatio: number | null;
  state: BudgetState | null;
}

export interface SourceUsage {
  sourceKey: string;
  label: SourceLabel;
  budgetable: boolean;
  intakeBytes: number | null;
  events: number | null;
  /** False while metering does not cover the whole window: the figures are then lower bounds. */
  complete: boolean;
  share: number | null;
  budget: IntakeBudget | null;
}

export interface UsageOverview {
  generatedAt: number;
  windowStart: number;
  windowEnd: number;
  windowSecs: number;
  dimension: UsageDimension;
  /** False when the figures could not be read: everything is unknown, not zero. */
  available: boolean;
  meteringStartedAt: number | null;
  totalBytes: number | null;
  totalEvents: number | null;
  /** Too many sources to list: those that took in the least were left out. */
  truncated: boolean;
  sources: SourceUsage[];
}

export interface UsageBucket {
  startTime: number;
  endTime: number;
  intakeBytes: number;
  events: number;
}

export interface UsageBreakdownRow {
  sourceKey: string;
  label: SourceLabel;
  intakeBytes: number;
  events: number;
}

export interface UsageDetail {
  generatedAt: number;
  windowStart: number;
  windowEnd: number;
  windowSecs: number;
  meteringStartedAt: number | null;
  intakeBytes: number;
  events: number;
  buckets: UsageBucket[];
  /** The source along the other two dimensions. */
  breakdowns: { dimension: UsageDimension; sources: UsageBreakdownRow[] }[];
}

export interface BudgetListItem extends IntakeBudget {
  label: SourceLabel;
}

export interface SetBudgetInput {
  dimension: UsageDimension;
  sourceKey: string;
  period: BudgetPeriod;
  limitSize: number;
  warnPercent: number;
  /** Left out, the person setting the budget is told. */
  notifyUserIds?: string[];
}

export async function overview(params: {
  dimension: UsageDimension;
  windowSecs: number;
}): Promise<UsageOverview> {
  const { data } = await http.get<UsageOverview>('/streams/usage', {
    params: { dimension: params.dimension, windowSecs: params.windowSecs },
  });
  return data;
}

export async function detail(params: {
  dimension: UsageDimension;
  sourceKey: string;
  windowSecs: number;
  bucketCount: number;
}): Promise<UsageDetail> {
  const { data } = await http.get<UsageDetail>('/streams/usage/detail', {
    params: {
      dimension: params.dimension,
      sourceKey: params.sourceKey,
      windowSecs: params.windowSecs,
      bucketCount: params.bucketCount,
    },
  });
  return data;
}

export async function listBudgets(): Promise<BudgetListItem[]> {
  const { data } = await http.get<{ budgets?: BudgetListItem[] }>('/streams/budgets');
  return data.budgets ?? [];
}

/** Sets the budget of a source. A source has one at most: setting it again changes that one. */
export async function setBudget(input: SetBudgetInput): Promise<IntakeBudget> {
  const { data } = await http.post<IntakeBudget>('/streams/budgets', input);
  return data;
}

export async function removeBudget(id: string): Promise<void> {
  await http.delete(`/streams/budgets/${encodeURIComponent(id)}`);
}
