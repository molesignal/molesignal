import type {
  RumAggregate,
  RumAggregateParams,
  RumFilter,
  RumIssue,
  RumIssueFilter,
  RumIssueSort,
  RumIssueStatus,
} from '@/api/rum';

import { type OverviewRange, type OverviewScope, scopeFilters } from '../overview/model';

/**
 * Each state in the ink of what it says, as the errors of APM have them, so that the product has
 * one set of states and one way to read them.
 */
export const STATE_INK: Record<RumIssueStatus, string> = {
  open: 'text-tx-1',
  investigating: 'text-[var(--blue-ink)]',
  regressed: 'text-[var(--red-ink)]',
  resolved: 'text-[var(--green-ink)]',
  ignored: 'text-tx-3',
};

/** What a list can be narrowed to: what still needs somebody (the default), a state, or everything. */
export const STATUS_FILTERS: readonly RumIssueFilter[] = ['unresolved', 'regressed', 'investigating', 'resolved', 'ignored', 'all'];

export const DEFAULT_FILTER: RumIssueFilter = 'unresolved';

export function parseFilter(value: string | null): RumIssueFilter {
  return STATUS_FILTERS.find((filter) => filter === value) ?? DEFAULT_FILTER;
}

export const SORTS: readonly RumIssueSort[] = ['sessions', 'users', 'occurrences', 'last_seen', 'first_seen', 'status'];

export const DEFAULT_SORT: RumIssueSort = 'sessions';

export function parseSort(value: string | null): RumIssueSort {
  return SORTS.find((sort) => sort === value) ?? DEFAULT_SORT;
}

/** The order a sort reads in when none is chosen: the biggest or the latest first. */
export const defaultOrder = (sort: RumIssueSort): 'asc' | 'desc' => (sort === 'status' ? 'asc' : 'desc');

export function parseOrder(value: string | null): 'asc' | 'desc' | undefined {
  return value === 'asc' || value === 'desc' ? value : undefined;
}

/** The issues a list shows at the most: the distributions beside it name them in one filter. */
export const LIST_LIMIT = 100;

/** The dimensions the distributions above the list spread the affected sessions over. */
export const DIMENSIONS = [
  { key: 'version', field: 'version', param: 'version' },
  { key: 'page', field: 'view.url_path_group', param: 'page' },
  { key: 'browser', field: 'browser.name', param: 'browser' },
] as const;

export type Dimension = (typeof DIMENSIONS)[number];
export type DimensionKey = Dimension['key'];

/** What is picked in the distributions, by dimension; a dimension that is not picked is left out. */
export type Picked = Partial<Record<DimensionKey, string>>;

/** The filters of what is picked, as conditions on the errors counted. */
export function pickedFilters(picked: Picked): RumFilter[] {
  return DIMENSIONS.flatMap(({ key, field }) => {
    const value = picked[key];

    return value ? [{ field, op: '=', value }] : [];
  });
}

const sessions = { fn: 'count_distinct', field: 'session.id', name: 'sessions' } as const;

const issuesIn = (ids: readonly string[]): RumFilter => ({ field: 'error.issue_id', op: 'in', value: JSON.stringify(ids) });

/**
 * The sessions of the errors of the listed issues, spread over the values of a dimension. What is
 * picked narrows the list and every distribution with it: a picked value stays in its own column to
 * be let go of.
 */
export function spreadQuery(
  scope: OverviewScope,
  range: OverviewRange,
  ids: readonly string[],
  picked: Picked,
  dimension: Dimension,
  extra: readonly RumFilter[] = [],
): RumAggregateParams {
  return {
    type: 'errors',
    from: range.from,
    to: range.to,
    filters: scopeFilters(scope, [issuesIn(ids), ...extra, ...pickedFilters(picked)]),
    measures: [sessions],
    groupBy: [dimension.field],
    orderBy: 'sessions',
    order: 'desc',
    limit: 250,
  };
}

export interface Spread {
  value: string;
  sessions: number;
}

/** What an answer spread the sessions over, the commonest value first; the rows with no value are left out. */
export function readSpread(answer: RumAggregate | undefined): Spread[] {
  return (answer?.rows ?? []).flatMap((row) => {
    const value = row.group[0];
    const count = row.values[0];

    return value && typeof count === 'number' ? [{ value, sessions: count }] : [];
  });
}

/** What an issue is spread over in its own drawer: only its errors count. */
export const WHERE = [
  { key: 'browser', field: 'browser.name' },
  { key: 'system', field: 'os.name' },
  { key: 'country', field: 'geo.country_iso_code' },
] as const;

export type WhereKey = (typeof WHERE)[number]['key'];

/** The sessions the errors of one issue met, spread over the values of a field. */
export function whereQuery(scope: OverviewScope, range: OverviewRange, id: string, field: string): RumAggregateParams {
  return {
    type: 'errors',
    from: range.from,
    to: range.to,
    filters: scopeFilters(scope, [{ field: 'error.issue_id', op: '=', value: id }]),
    measures: [sessions],
    groupBy: [field],
    orderBy: 'sessions',
    order: 'desc',
    limit: 250,
  };
}

const DAY_MICROS = 24 * 60 * 60 * 1_000_000;

/** The days the trend of an issue looks back over, today included. */
export const TREND_DAYS = 14;

/** The range of the trend of an issue: whole local days, the last being today. */
export function trendRange(now: Date): OverviewRange {
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (TREND_DAYS - 1)).getTime() * 1_000;

  return { from: start, to: now.getTime() * 1_000 };
}

/** The sessions an issue met, by version and by day, over the range of the trend. */
export function trendQuery(scope: OverviewScope, range: OverviewRange, id: string): RumAggregateParams {
  return {
    type: 'errors',
    from: range.from,
    to: range.to,
    filters: scopeFilters(scope, [{ field: 'error.issue_id', op: '=', value: id }]),
    measures: [sessions],
    groupBy: ['version'],
    bucketTime: DAY_MICROS,
    originTime: range.from,
    orderBy: 'sessions',
    order: 'desc',
    limit: 20,
  };
}

/** A stack of bars: the versions that count, and the rest together. */
export interface Trend {
  /** The start of each day, in microseconds. */
  days: number[];
  series: Array<{ version: string | null; counts: number[] }>;
}

/** The most versions a trend draws in a colour of their own; the rest are drawn as one. */
export const TREND_SERIES = 5;

/** The trend of an answer: a series for each version in the order they began, the least frequent merged. */
export function readTrend(answer: RumAggregate | undefined, range: OverviewRange): Trend | null {
  if (!answer || answer.unavailable) return null;

  const days = Array.from({ length: TREND_DAYS }, (_, index) => range.from + index * DAY_MICROS);
  const byVersion = new Map<string | null, number[]>();

  for (const row of answer.rows) {
    const count = row.values[0];
    const start = row.startTime;

    if (typeof count !== 'number' || start === undefined) continue;

    const day = Math.floor((start - range.from) / DAY_MICROS);

    if (day < 0 || day >= TREND_DAYS) continue;

    const key = row.group[0] ?? null;
    const counts = byVersion.get(key) ?? new Array<number>(TREND_DAYS).fill(0);

    counts[day] = (counts[day] ?? 0) + count;
    byVersion.set(key, counts);
  }

  const total = (counts: number[]) => counts.reduce((sum, count) => sum + count, 0);
  const firstDay = (counts: number[]) => counts.findIndex((count) => count > 0);

  const ranked = [...byVersion.entries()].sort((left, right) => total(right[1]) - total(left[1]));
  const kept = ranked.slice(0, TREND_SERIES);
  const rest = ranked.slice(TREND_SERIES);

  // The versions in the order they began: the oldest at the bottom of a bar.
  kept.sort((left, right) => firstDay(left[1]) - firstDay(right[1]) || String(left[0]).localeCompare(String(right[0])));

  const series = kept.map(([version, counts]) => ({ version, counts }));

  if (rest.length > 0) {
    const merged = new Array<number>(TREND_DAYS).fill(0);

    for (const [, counts] of rest) counts.forEach((count, day) => (merged[day] = (merged[day] ?? 0) + count));
    series.unshift({ version: '', counts: merged });
  }

  return { days, series };
}

/** The most people the list of the people an issue met names. */
export const PEOPLE_SHOWN = 5;

/** What the people an issue met are asked as: those with an id, and those without, apart. */
export function peopleQueries(scope: OverviewScope, range: OverviewRange, id: string): { known: RumAggregateParams; anonymous: RumAggregateParams } {
  const own: RumFilter = { field: 'error.issue_id', op: '=', value: id };

  const asked = (filter: RumFilter, group: string): RumAggregateParams => ({
    type: 'errors',
    from: range.from,
    to: range.to,
    filters: scopeFilters(scope, [own, filter]),
    measures: [{ fn: 'count', name: 'occurrences' }],
    groupBy: [group],
    orderBy: 'occurrences',
    order: 'desc',
    limit: PEOPLE_SHOWN,
  });

  return {
    known: asked({ field: 'usr.id', op: 'exists', value: '' }, 'usr.id'),
    anonymous: asked({ field: 'usr.id', op: 'missing', value: '' }, 'usr.anonymous_id'),
  };
}

export interface Person {
  /** The id, or the anonymous id of a visitor who never signed in. */
  id: string;
  anonymous: boolean;
  occurrences: number;
}

export function readPeople(known: RumAggregate | undefined, anonymous: RumAggregate | undefined): Person[] {
  const rows = (answer: RumAggregate | undefined, isAnonymous: boolean): Person[] =>
    (answer?.rows ?? []).flatMap((row) => {
      const id = row.group[0];
      const count = row.values[0];

      return id && typeof count === 'number' ? [{ id, anonymous: isAnonymous, occurrences: count }] : [];
    });

  return [...rows(known, false), ...rows(anonymous, true)].sort((left, right) => right.occurrences - left.occurrences).slice(0, PEOPLE_SHOWN);
}

/** The place an issue was seen first and last in, as `2.2.0 → 2.4.1`; one version alone when they are the same. */
export function versionSpan(issue: Pick<RumIssue, 'firstVersion' | 'lastVersion'>): string {
  const { firstVersion: first, lastVersion: last } = issue;

  if (first && last && first !== last) return `${first} → ${last}`;

  return last ?? first ?? '';
}

/** How long ago a moment was, in the words of the language: "3 minutes ago", "yesterday". */
export function formatAgo(micros: number, now: number, language: string): string {
  const seconds = Math.round((micros - now) / 1_000_000);
  const format = new Intl.RelativeTimeFormat(language, { numeric: 'auto', style: 'short' });
  const past = Math.abs(seconds);

  if (past < 45) return format.format(0, 'second');
  if (past < 45 * 60) return format.format(Math.round(seconds / 60), 'minute');
  if (past < 22 * 3600) return format.format(Math.round(seconds / 3600), 'hour');

  return format.format(Math.round(seconds / 86_400), 'day');
}
