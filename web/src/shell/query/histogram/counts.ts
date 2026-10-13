import { keepPreviousData, useQuery } from '@tanstack/react-query';

import type { HistogramCount, HistogramPlan, HistogramRange } from './model';

/** What is asked to be counted: a range, into bars a chart has already settled on. */
export interface HistogramCountsRequest {
  /** The range the results were asked from, in microseconds. */
  range: HistogramRange;
  /** What one bar stands for, in microseconds. */
  binMicros: number;
  /** Where the first bar begins: at or before the start of the range. */
  originMicros: number;
}

/**
 * Why there are no counts of everything a query matches. All but the last are
 * said by the backend; `failed` is a request that did not come back.
 */
export type HistogramUnavailable = 'time_budget' | 'busy' | 'scan_limit' | 'timestamp_masked' | 'failed';

export const HISTOGRAM_UNAVAILABLE: readonly HistogramUnavailable[] = [
  'time_budget',
  'busy',
  'scan_limit',
  'timestamp_masked',
  'failed',
];

export type HistogramCountsAnswer =
  | {
      bars: HistogramCount[];
      /** Whether errors were counted apart from the rest. */
      split: boolean;
    }
  | { unavailable: HistogramUnavailable };

/** Where a chart gets the counts of everything a query matches, not only the loaded page. */
export interface HistogramCountsSource {
  /**
   * Names the query that is counted. Another query, or the same one run again,
   * has another key and is counted anew; the same key is counted once.
   */
  key: readonly unknown[];
  load: (request: HistogramCountsRequest, signal: AbortSignal) => Promise<HistogramCountsAnswer>;
}

/** Counts as the web endpoints answer them. */
export interface HistogramCountsPayload {
  buckets?: Array<{ start_micros: number; total: number; errors: number }> | null;
  split?: boolean;
  unavailable?: string | null;
}

/** Reads the answer of a counts endpoint. An answer that is not counts at all counts as a failure. */
export function answerFromPayload(payload: HistogramCountsPayload | null | undefined): HistogramCountsAnswer {
  const unavailable = HISTOGRAM_UNAVAILABLE.find((reason) => reason === payload?.unavailable);
  if (unavailable) return { unavailable };
  if (!payload || payload.unavailable || !Array.isArray(payload.buckets)) return { unavailable: 'failed' };

  return {
    bars: payload.buckets.map((bucket) => ({
      startMicros: bucket.start_micros,
      total: bucket.total,
      errors: bucket.errors,
    })),
    split: payload.split !== false,
  };
}

export interface CountedHistogram {
  /** The bars the answer was counted for. */
  plan: HistogramPlan;
  answer: HistogramCountsAnswer;
}

export interface HistogramCountsState {
  /**
   * The answer on hand. While `pending`, it is the one before: of the query
   * that ran last, or of the same query in other bars.
   */
  counted: CountedHistogram | null;
  pending: boolean;
}

const NOT_COUNTED: HistogramCountsState = { counted: null, pending: false };

/**
 * Asks `source` for the counts of the bars in `plan`, once for each query and
 * each set of bars, and keeps the answer before on hand while the next is on
 * its way. Nothing is asked without a source or a plan, or while `ready` is
 * false: the bars are not settled until the chart has been measured.
 */
export function useHistogramCounts(
  source: HistogramCountsSource | undefined,
  range: HistogramRange | null,
  plan: HistogramPlan | null,
  ready: boolean,
): HistogramCountsState {
  const asked = Boolean(source && range && plan);

  const query = useQuery({
    queryKey: [
      'histogram-counts',
      ...(source?.key ?? []),
      range?.start,
      range?.end,
      plan?.binMicros,
      plan?.frame.start,
    ],
    queryFn: async ({ signal }): Promise<CountedHistogram> => {
      if (!source || !range || !plan) throw new Error('nothing to count');

      try {
        const request = { range, binMicros: plan.binMicros, originMicros: plan.frame.start };

        return { plan, answer: await source.load(request, signal) };
      } catch (error) {
        // A request that was given up belongs to no chart any more.
        if (signal.aborted) throw error;

        return { plan, answer: { unavailable: 'failed' } };
      }
    },
    enabled: asked && ready,
    placeholderData: keepPreviousData,
    // An answer stands for the run it was asked for; the next run has its own key.
    staleTime: Infinity,
    retry: false,
  });

  if (!asked) return NOT_COUNTED;

  return { counted: query.data ?? null, pending: query.isPending || query.isPlaceholderData };
}
