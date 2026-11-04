import { getOrPost } from '@/lib/http';

import type { RumFilter, RumQuery, RumUnit } from './explorer';

/**
 * What a measure does to the rows it takes: the percentiles are estimates. With `estimate` a count,
 * a sum or an average is made of what the rows stand for; `stored` is always the rows that are stored.
 */
export type RumMeasureFn =
  | 'count'
  | 'stored'
  | 'count_distinct'
  | 'sum'
  | 'avg'
  | 'min'
  | 'max'
  | 'p50'
  | 'p75'
  | 'p90'
  | 'p95'
  | 'p99';

export interface RumMeasure {
  fn: RumMeasureFn;
  /** `count` without one counts rows; with one, the rows that have a value in it. */
  field?: string;
  /** For `count_distinct`: the field to take where `field` has no value. */
  fallback?: string;
  /** The measure takes only the rows that meet these. */
  where?: RumFilter[];
  /** What the answer calls it; `fn(field)`, or `fn` without a field, when left out. */
  name?: string;
}

export interface RumAggregateParams extends RumQuery {
  measures: RumMeasure[];
  /** Up to two fields: the answer has a row per combination of their values. */
  groupBy?: string[];
  /** With bars of time the answer has a row per group per bar. */
  bucketTime?: number;
  originTime?: number;
  /** A measure's name, a group field, or `time`. */
  orderBy?: string;
  order?: 'asc' | 'desc';
  /** How many groups to answer with. */
  limit?: number;
  /**
   * Count what the rows that were kept stand for, by the share each was kept with, so that totals are
   * those of every session; `false` counts the rows that are stored. The server estimates when it is left out.
   */
  estimate?: boolean;
}

export interface RumAggregateRow {
  /** What the row is of, in the order of `groupBy`; `null` where the rows have no value. */
  group: Array<string | null>;
  /** Where the bar of the row begins, in microseconds, in an answer over time. */
  startTime?: number;
  /** In the order of the measures. A value the reader may not see is the text it is masked to. */
  values: Array<number | string | null>;
}

export interface RumAggregate {
  groupBy: string[];
  measures: Array<{ name: string; fn: RumMeasureFn; field?: string; unit: RumUnit }>;
  bucketTime?: number;
  originTime?: number;
  rows: RumAggregateRow[];
  unavailable?: 'time_budget' | 'busy' | 'timestamp_masked';
}

const json = (list: unknown[] | undefined) => (list && list.length > 0 ? JSON.stringify(list) : undefined);

/**
 * Numbers over the rows the query matches: counts, sums, averages and percentiles, per group of up
 * to two fields and, with bars, per bar of time. Said in its address like the other reads, so a
 * chart's request can be copied and opened again.
 */
export async function aggregate(params: RumAggregateParams, signal?: AbortSignal): Promise<RumAggregate> {
  const { filters, freeText, measures, groupBy, ...rest } = params;

  return getOrPost<RumAggregate>(
    '/rum/aggregate',
    {
      params: {
        ...rest,
        filters: json(filters),
        freeText: json(freeText),
        measures: json(measures),
        groupBy: json(groupBy),
      },
      body: params,
    },
    { ...(signal && { signal }) },
  );
}
