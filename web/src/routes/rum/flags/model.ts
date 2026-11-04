import type { RumAggregate, RumAggregateParams, RumFilter, RumMeasure } from '@/api/rum';

import { type OverviewRange, type OverviewScope, scopeFilters } from '../overview/model';

/** The column a flag is asked by: a key of the feature flags an event carries. */
export const flagField = (flag: string): string => `feature_flags.${flag}`;

const over = (field: string): RumFilter[] => [{ field, op: '>', value: '0' }];

const MEASURES: readonly RumMeasure[] = [
  { fn: 'count_distinct', field: 'session.id', name: 'sessions' },
  { fn: 'count', name: 'views' },
  { fn: 'count', name: 'with_error', where: over('view.error.count') },
  { fn: 'count', name: 'frustrated', where: over('view.frustration.count') },
  { fn: 'p75', field: 'view.performance.lcp.timestamp', name: 'lcp' },
  { fn: 'p75', field: 'view.performance.inp.duration', name: 'inp' },
  { fn: 'p75', field: 'view.performance.cls.score', name: 'cls' },
];

/** The read that compares the values of a flag: the views of each, as the page of a vital counts them. */
export function comparisonQuery(scope: OverviewScope, range: OverviewRange, flag: string): RumAggregateParams {
  return {
    type: 'views',
    from: range.from,
    to: range.to,
    filters: scopeFilters(scope),
    measures: [...MEASURES],
    groupBy: [flagField(flag)],
    orderBy: 'sessions',
    order: 'desc',
    limit: 20,
    estimate: true,
  };
}

/** How the sessions of one value of a flag did. */
export interface Variant {
  /** What the flag said; null for the views that had not evaluated it. */
  value: string | null;
  sessions: number;
  views: number;
  /** Shares of the views, from 0 to 1. */
  errorShare: number | null;
  frustrationShare: number | null;
  /** In nanoseconds, as the events say them. */
  lcp: number | null;
  inp: number | null;
  cls: number | null;
}

const numberOf = (value: unknown): number | null => (typeof value === 'number' && Number.isFinite(value) ? value : null);

export function readVariants(answer: RumAggregate | undefined): Variant[] {
  if (!answer) return [];

  const at = (name: string) => answer.measures.findIndex((measure) => measure.name === name);

  return answer.rows.map((row) => {
    const of = (name: string) => numberOf(row.values[at(name)]);
    const views = of('views') ?? 0;
    const share = (name: string) => {
      const part = of(name);

      return part === null || views === 0 ? null : part / views;
    };

    return {
      value: row.group[0] ?? null,
      sessions: of('sessions') ?? 0,
      views,
      errorShare: share('with_error'),
      frustrationShare: share('frustrated'),
      lcp: of('lcp'),
      inp: of('inp'),
      cls: of('cls'),
    };
  });
}
