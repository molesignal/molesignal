import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import type { FieldAnalysis, FieldAnalysisKind } from './model';

/**
 * Why a field has no analysis of everything a query matches. All but the last
 * are said by the backend; `failed` is a request that did not come back.
 */
export type FieldAnalysisUnavailable = 'time_budget' | 'busy' | 'field_protected' | 'failed';

const UNAVAILABLE: readonly FieldAnalysisUnavailable[] = ['time_budget', 'busy', 'field_protected', 'failed'];
const KINDS: readonly FieldAnalysisKind[] = ['number', 'time', 'text', 'bool'];

export type FieldAnalysisAnswer = { analysis: FieldAnalysis } | { unavailable: FieldAnalysisUnavailable };

/** Where the analysis of everything a query matches comes from, not only of the loaded page. */
export interface FieldAnalysisSource {
  /**
   * Names the field and the query it is read across. Another field, another
   * query, or the same one run again has another key and is read anew.
   */
  key: readonly unknown[];
  load: (signal: AbortSignal) => Promise<FieldAnalysisAnswer>;
}

/** An analysis as the web endpoints answer it. */
export interface FieldAnalysisPayload {
  kind?: string;
  total?: number;
  present?: number;
  distinct?: number;
  distinct_exact?: boolean;
  range?: { min: string; max: string } | null;
  spread?: {
    avg?: number | null;
    q1?: number | null;
    median?: number | null;
    q3?: number | null;
    stddev_samp?: number | null;
    stddev_pop?: number | null;
    skewness?: number | null;
    kurtosis?: number | null;
  } | null;
  bars?: { origin: number; width: number; counts: number[] } | null;
  top?: Array<{ value: string; count: number }> | null;
  top_skipped?: boolean;
  trend?: {
    origin_micros: number;
    bucket_micros: number;
    series: Array<{ value: string; counts: number[] }>;
  } | null;
  unavailable?: string | null;
}

const count = (value: unknown): number => (typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : 0);
const statistic = (value: unknown): number | null => (typeof value === 'number' && Number.isFinite(value) ? value : null);

/** Reads the answer of an analysis endpoint. An answer that is not an analysis at all counts as a failure. */
export function answerFromPayload(payload: FieldAnalysisPayload | null | undefined): FieldAnalysisAnswer {
  const unavailable = UNAVAILABLE.find((reason) => reason === payload?.unavailable);
  if (unavailable) return { unavailable };

  const kind = KINDS.find((candidate) => candidate === payload?.kind);
  if (!payload || payload.unavailable || !kind || typeof payload.total !== 'number') return { unavailable: 'failed' };

  const { range, spread, bars, trend } = payload;

  return {
    analysis: {
      kind,
      total: count(payload.total),
      present: count(payload.present),
      distinct: count(payload.distinct),
      distinctExact: payload.distinct_exact === true,
      range: range && typeof range.min === 'string' && typeof range.max === 'string' ? { min: range.min, max: range.max } : null,
      spread: spread
        ? {
            avg: statistic(spread.avg),
            q1: statistic(spread.q1),
            median: statistic(spread.median),
            q3: statistic(spread.q3),
            stddevSamp: statistic(spread.stddev_samp),
            stddevPop: statistic(spread.stddev_pop),
            skewness: statistic(spread.skewness),
            kurtosis: statistic(spread.kurtosis),
          }
        : null,
      bars:
        bars && Number.isFinite(bars.origin) && bars.width > 0 && Array.isArray(bars.counts) && bars.counts.length > 0
          ? { origin: bars.origin, width: bars.width, counts: bars.counts.map(count) }
          : null,
      top: (payload.top ?? []).map((item) => ({ value: String(item.value), count: count(item.count) })),
      topSkipped: payload.top_skipped === true,
      trend:
        trend && Number.isFinite(trend.origin_micros) && trend.bucket_micros > 0 && Array.isArray(trend.series) && trend.series.length > 0
          ? {
              originMicros: trend.origin_micros,
              bucketMicros: trend.bucket_micros,
              series: trend.series.map((item) => ({ value: String(item.value), counts: item.counts.map(count) })),
            }
          : null,
    },
  };
}

/** What the rows were that an analysis is of: everything the query matches, or what is loaded. */
export type FieldAnalysisScope = 'all' | 'loaded';

export interface FieldAnalysisState {
  /** `null` while it is being read, and for a field whose values are not the reader's to count. */
  analysis: FieldAnalysis | null;
  scope: FieldAnalysisScope;
  pending: boolean;
  /**
   * Why the analysis is not of everything the query matches, when a source was
   * asked for it. With `field_protected` there is no analysis at all.
   */
  unavailable: FieldAnalysisUnavailable | null;
  retry: () => void;
}

/**
 * The analysis of a field: of everything the query matches where `source` can
 * read that, and of the loaded results (`loaded`) where there is no source or
 * it could not answer. `loaded` answers `null` where the loaded rows do not
 * hold the field at all. A field that is masked or encrypted has neither: what
 * is loaded of it is masked too, and counting masks says nothing.
 */
export function useFieldAnalysis(
  source: FieldAnalysisSource | undefined,
  loaded: () => FieldAnalysis | null,
): FieldAnalysisState {
  const query = useQuery({
    queryKey: ['field-analysis', ...(source?.key ?? [])],
    queryFn: async ({ signal }): Promise<FieldAnalysisAnswer> => {
      if (!source) throw new Error('nothing to analyse');

      try {
        return await source.load(signal);
      } catch (error) {
        // A request that was given up belongs to no panel any more.
        if (signal.aborted) throw error;

        return { unavailable: 'failed' };
      }
    },
    enabled: Boolean(source),
    // An answer stands for the run it was asked for; the next run has its own key.
    staleTime: Infinity,
    retry: false,
  });
  const answer = source ? query.data : undefined;
  const whole = answer && 'analysis' in answer ? answer.analysis : null;
  const unavailable = answer && 'unavailable' in answer ? answer.unavailable : null;
  const pending = Boolean(source) && query.isPending;
  const fallsBack = !pending && !whole && unavailable !== 'field_protected';
  // Worked out only when it is what is shown: it walks every loaded row.
  const local = React.useMemo(() => (fallsBack ? loaded() : null), [fallsBack, loaded]);

  return {
    analysis: whole ?? local,
    // While it is being read, what is asked for is everything.
    scope: whole || pending ? 'all' : 'loaded',
    pending,
    unavailable,
    retry: () => void query.refetch(),
  };
}
