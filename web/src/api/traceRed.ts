import { http } from '@/lib/http';

import type { HistogramBars, TraceCondition, TraceFilter } from './web';

/**
 * Which Spans are counted as requests: where a service took one in (a server
 * or consumer Span, or the root of a trace), or every Span.
 */
export type TraceRedScope = 'entry' | 'all';

/** Why there are no figures, as the backend says it. */
export type TraceRedUnavailable =
  | 'time_budget'
  | 'busy'
  | 'scan_limit'
  | 'timestamp_masked'
  | 'field_protected';

/** Rate, errors and duration of a set of requests. */
export interface TraceRedFigures {
  requests: number;
  /** `null` where failures cannot be told: no status, or a masked one. */
  errors: number | null;
  /** How long the requests took, in nanoseconds; `null` without a request or where durations are masked. */
  p50Ns: number | null;
  p95Ns: number | null;
  p99Ns: number | null;
}

export interface TraceRedBucket extends TraceRedFigures {
  startTime: number;
}

/** How many requests took from `from_ns` up to `to_ns`; the last bar has no end. */
export interface TraceRedLatencyBar {
  fromNs: number;
  toNs: number | null;
  requests: number;
  errors: number | null;
}

export interface TraceRedResponse {
  bucketTime: number;
  originTime: number;
  scope: TraceRedScope;
  total: TraceRedFigures;
  /** The bars that hold a request, oldest first. */
  buckets: TraceRedBucket[];
  /** The bars of durations that hold a request, shortest first. */
  latency: TraceRedLatencyBar[];
  unavailable?: TraceRedUnavailable;
}

export interface TraceRedPoint {
  startTime: number;
  requests: number;
  errors: number | null;
  p95Ns: number | null;
}

export interface TraceRedGroup extends TraceRedFigures {
  /** `null` for the requests without a value. */
  value: string | null;
  /** The bars that hold one of these requests, oldest first. */
  series: TraceRedPoint[];
}

export interface TraceRedBreakdownResponse {
  field: string;
  bucketTime: number;
  originTime: number;
  scope: TraceRedScope;
  /** The commonest values first. */
  groups: TraceRedGroup[];
  /** More values than are listed. */
  more: boolean;
  unavailable?: TraceRedUnavailable;
}

/** The requests a RED figure is of: the list query's own filters and free text, in a scope. */
export interface TraceRedParams extends HistogramBars {
  from: number;
  to: number;
  q?: string;
  filters?: TraceFilter[];
  where?: TraceCondition;
  scope: TraceRedScope;
}

function requestParams({ filters, where, ...rest }: TraceRedParams) {
  return {
    ...rest,
    ...(filters && filters.length > 0 ? { filters: JSON.stringify(filters) } : {}),
    ...(where ? { where: JSON.stringify(where) } : {}),
  };
}

/** Rate, errors and duration of the requests the trace query matches, bar by bar of the range. */
export async function tracesRed(params: TraceRedParams, signal?: AbortSignal): Promise<TraceRedResponse> {
  const { data } = await http.get<TraceRedResponse>('/web/traces/red', {
    params: requestParams(params),
    ...(signal && { signal }),
  });
  return data;
}

/** The same requests grouped by the values of `by`, the commonest first. */
export async function tracesRedBreakdown(
  params: TraceRedParams & { by: string },
  signal?: AbortSignal,
): Promise<TraceRedBreakdownResponse> {
  const { data } = await http.get<TraceRedBreakdownResponse>('/web/traces/red/breakdown', {
    params: requestParams(params),
    ...(signal && { signal }),
  });
  return data;
}
