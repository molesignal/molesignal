import type { TFunction } from 'i18next';

import type {
  BudgetState,
  IntakeBudget,
  SourceLabel,
  SourceUsage,
  UsageDimension,
} from '@/api/intakeUsage';
import type { StatusTone } from '@/shell/StatusWord';

/** What a figure that is not known looks like: never zero. */
export const UNKNOWN_FIGURE = '–';

export const USAGE_DIMENSIONS = ['credential', 'stream', 'service'] as const satisfies readonly UsageDimension[];

export const USAGE_WINDOWS = [
  { key: '24h', secs: 24 * 60 * 60, buckets: 24 },
  { key: '7d', secs: 7 * 24 * 60 * 60, buckets: 28 },
  { key: '30d', secs: 30 * 24 * 60 * 60, buckets: 30 },
] as const;

export type UsageWindowKey = (typeof USAGE_WINDOWS)[number]['key'];

export function dimensionFromParam(value: string | null): UsageDimension {
  return USAGE_DIMENSIONS.find((dimension) => dimension === value) ?? 'credential';
}

export function windowFromParam(value: string | null): (typeof USAGE_WINDOWS)[number] {
  return USAGE_WINDOWS.find((window) => window.key === value) ?? USAGE_WINDOWS[0];
}

const BYTE_UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'] as const;

export function formatBytes(bytes: number): string {
  const unit = Math.min(Math.floor(Math.log2(Math.max(bytes, 1)) / 10), BYTE_UNITS.length - 1);
  const value = bytes / 1024 ** unit;
  return `${value.toLocaleString(undefined, { maximumFractionDigits: unit === 0 ? 0 : 2 })} ${BYTE_UNITS[unit]}`;
}

/** Bytes as a figure: unknown as such, a lower bound with its mark. */
export function bytesFigure(bytes: number | null | undefined, complete = true): string {
  if (bytes == null || !Number.isFinite(bytes)) return UNKNOWN_FIGURE;
  return `${complete ? '' : '≥ '}${formatBytes(bytes)}`;
}

export function countFigure(count: number | null | undefined, complete = true): string {
  if (count == null || !Number.isFinite(count)) return UNKNOWN_FIGURE;
  return `${complete ? '' : '≥ '}${count.toLocaleString()}`;
}

/** A share of the total. A sliver is said to be one rather than rounded to nothing. */
export function shareFigure(share: number | null | undefined): string {
  if (share == null || !Number.isFinite(share)) return UNKNOWN_FIGURE;
  if (share > 0 && share < 0.001) return '< 0.1%';
  return `${(share * 100).toLocaleString(undefined, { maximumFractionDigits: 1 })}%`;
}

/** How much of a budget is used, in whole percents, rounded down so that 100% means used up. */
export function usedFigure(budget: Pick<IntakeBudget, 'usedRatio' | 'usedComplete'> | null | undefined): string {
  if (budget?.usedRatio == null || !Number.isFinite(budget.usedRatio)) return UNKNOWN_FIGURE;
  return `${budget.usedComplete ? '' : '≥ '}${Math.floor(budget.usedRatio * 100).toLocaleString()}%`;
}

export const LIMIT_UNITS = ['MiB', 'GiB', 'TiB'] as const;
export type LimitUnit = (typeof LIMIT_UNITS)[number];

const MIB_PER_UNIT: Record<LimitUnit, number> = { MiB: 1, GiB: 1024, TiB: 1024 * 1024 };

/** The largest budget, in MiB: 1 PiB. Same as the backend (`MAX_BUDGET_LIMIT_SIZE`). */
export const MAX_LIMIT_SIZE = 1024 * 1024 * 1024;

/** What was typed as a limit, in whole MiB, or `null` when it is not a limit. */
export function parseLimit(text: string, unit: LimitUnit): number | null {
  const trimmed = text.trim();
  if (!/^\d+(\.\d+)?$/.test(trimmed)) return null;

  const size = Math.round(Number(trimmed) * MIB_PER_UNIT[unit]);
  return size >= 1 && size <= MAX_LIMIT_SIZE ? size : null;
}

/** A limit in the largest unit that says it in a whole number. */
export function limitParts(limitSize: number): { text: string; unit: LimitUnit } {
  const unit = [...LIMIT_UNITS].reverse().find((candidate) => limitSize % MIB_PER_UNIT[candidate] === 0) ?? 'MiB';
  return { text: String(limitSize / MIB_PER_UNIT[unit]), unit };
}

export function limitFigure(limitSize: number): string {
  const { text, unit } = limitParts(limitSize);
  return `${Number(text).toLocaleString()} ${unit}`;
}

/** The percentage to warn at, or `null` when it is not one. Same bounds as the backend. */
export function parseWarnPercent(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;

  const percent = Number(trimmed);
  return percent <= 99 ? percent : null;
}

export const BUDGET_STATE_TONE: Record<BudgetState, StatusTone> = {
  ok: 'green',
  warning: 'orange',
  exceeded: 'red',
};

/** What wants attention first; what has no budget last. */
export function budgetStateRank(state: BudgetState | null | undefined): number | null {
  if (state === 'exceeded') return 0;
  if (state === 'warning') return 1;
  if (state === 'ok') return 2;
  return null;
}

type UsageT = TFunction<'streams'>;

/** The name of a source, as a person reads it. */
export function sourceName(t: UsageT, sourceKey: string, label: SourceLabel): string {
  if (label.kind === 'internal') return t('usage.source.internal');

  if (label.kind === 'service') {
    if (sourceKey === '') return t('usage.source.unnamed_service');
    if (sourceKey === '*') return t('usage.source.other_services');
  }

  return label.name;
}

/** What kind of source it is, and what tells it apart from others of that name. */
export function sourceCaption(t: UsageT, label: SourceLabel): string {
  const kind =
    label.kind === 'api_token'
      ? t(`usage.token_kinds.${label.subkind ?? 'personal'}`, { defaultValue: label.subkind ?? '' })
      : label.kind === 'connector'
        ? t('usage.source.connector', { kind: label.subkind ?? '' })
        : label.kind === 'user'
          ? t('usage.source.user')
          : label.kind === 'stream' && label.subkind
            ? t(`list.tabs.${label.subkind}`, { defaultValue: label.subkind })
            : '';

  return [kind, label.detail].filter(Boolean).join(' · ');
}

/** The way a source is matched by the search of the list. */
export function matchesSearch(t: UsageT, row: SourceUsage, search: string): boolean {
  const wanted = search.trim().toLowerCase();
  if (wanted === '') return true;

  return `${sourceName(t, row.sourceKey, row.label)} ${sourceCaption(t, row.label)} ${row.sourceKey}`
    .toLowerCase()
    .includes(wanted);
}

/** How the budget of a source reads: "500 GiB / day". */
export function budgetFigure(t: UsageT, budget: Pick<IntakeBudget, 'limitSize' | 'period'>): string {
  return t(`usage.budget.per_${budget.period}`, { limit: limitFigure(budget.limitSize) });
}
