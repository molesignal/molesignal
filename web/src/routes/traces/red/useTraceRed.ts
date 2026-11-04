import { keepPreviousData, useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as redApi from '@/api/traceRed';
import type { TraceCondition, TraceFilter } from '@/api/web';
import { useTimeFormatter } from '@/lib/time';
import { utcOffsetMicros } from '@/shell/query/histogram/axis';
import { planHistogram } from '@/shell/query/histogram/model';

/** About as many bars as a third of a wide page draws legibly. */
const RED_BARS = 60;

/** The bars the figures are laid out in: each `bucketTime` long, the first beginning at `originTime`. */
export interface RedBars {
  originTime: number;
  bucketTime: number;
  count: number;
}

/** What the figures are counted over: the query on screen, its range, and which Spans are requests. */
export interface RedRequest {
  orgId: string;
  /** The range the query covers, as instants. */
  range: { from: string; to: string };
  q?: string | undefined;
  filters: TraceFilter[];
  /** The query's alternatives, beside its filters. */
  where?: TraceCondition | undefined;
  scope: redApi.TraceRedScope;
  /** Goes up when the reader asks for the results again. */
  epoch: number;
  /** Off while the query is a statement: it has no requests to count. */
  enabled: boolean;
}

/**
 * The bars of the range, of a length a reader counts in and beginning on
 * their clock, settled before anything is asked so every chart of the view
 * lays its figures out in the same bars.
 */
export function useRedBars(range: { from: string; to: string }): RedBars & { from: number; to: number } {
  const { tz } = useTimeFormatter();

  return React.useMemo(() => {
    const from = Date.parse(range.from) * 1_000;
    const to = Math.max(Date.parse(range.to) * 1_000, from + 1);
    const plan = planHistogram(
      { start: from, end: to },
      { maxBins: RED_BARS, utcOffsetMicros: utcOffsetMicros(from, tz) },
    );

    return {
      from,
      to,
      originTime: plan.frame.start,
      bucketTime: plan.binMicros,
      count: Math.round((plan.frame.end - plan.frame.start) / plan.binMicros),
    };
  }, [range.from, range.to, tz]);
}

function params(request: RedRequest, bars: RedBars & { from: number; to: number }): redApi.TraceRedParams {
  return {
    from: bars.from,
    to: bars.to,
    bucketTime: bars.bucketTime,
    originTime: bars.originTime,
    scope: request.scope,
    ...(request.q ? { q: request.q } : {}),
    ...(request.filters.length > 0 ? { filters: request.filters } : {}),
    ...(request.where ? { where: request.where } : {}),
  };
}

function requestKey(request: RedRequest, bars: RedBars & { from: number; to: number }) {
  return [
    request.orgId,
    bars.from,
    bars.to,
    bars.bucketTime,
    bars.originTime,
    request.scope,
    request.q ?? '',
    JSON.stringify(request.filters),
    JSON.stringify(request.where ?? null),
    request.epoch,
  ] as const;
}

/** Rate, errors and duration of the requests, for the range and bar by bar. */
export function useRedOverview(request: RedRequest, bars: RedBars & { from: number; to: number }) {
  return useQuery({
    queryKey: ['traces', 'red', ...requestKey(request, bars)],
    queryFn: ({ signal }) => redApi.tracesRed(params(request, bars), signal),
    enabled: request.enabled && Boolean(request.orgId),
    // The figures of the query before stay up while the next ones are read.
    placeholderData: keepPreviousData,
    retry: false,
  });
}

/** The same requests by the values of `by`, the commonest first. */
export function useRedBreakdown(
  request: RedRequest,
  bars: RedBars & { from: number; to: number },
  by: string | null,
) {
  return useQuery({
    queryKey: ['traces', 'red', 'breakdown', by, ...requestKey(request, bars)],
    queryFn: ({ signal }) => redApi.tracesRedBreakdown({ ...params(request, bars), by: by ?? '' }, signal),
    enabled: request.enabled && Boolean(request.orgId) && Boolean(by),
    placeholderData: keepPreviousData,
    retry: false,
  });
}
