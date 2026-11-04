import { http } from '@/lib/http';
import type { CursorPage } from '@/pagination/cursor';

import type { TraceCondition, TraceFilter, TraceWindow } from './web';

/** The order the span list is in. */
export type SpanListSort = 'latest' | 'earliest' | 'duration_desc' | 'duration_asc';

/** One span the query matches, as the span list shows it. */
export interface SpanListItem {
  traceId: string;
  spanId: string;
  parentSpanId?: string;
  service: string;
  operation: string;
  startNs: number;
  durationNs: number;
  /** `OK`, `ERROR` or `UNSET`. */
  status: string;
  /** As OTLP numbers it: 1 internal, 2 server, 3 client, 4 producer, 5 consumer. */
  kind?: number;
  httpStatusCode?: number;
}

/**
 * The spans a trace query matches, a page at a time. A filter on a field of
 * a span holds for the span itself; `duration_ns` is the span's own.
 */
export async function traceSpans(params: {
  from?: number;
  to?: number;
  limit?: number;
  q?: string;
  filters?: TraceFilter[];
  where?: TraceCondition;
  sort?: SpanListSort;
  cursor?: string;
}): Promise<CursorPage<SpanListItem>> {
  const { filters, where, ...rest } = params;
  const { data } = await http.get<CursorPage<SpanListItem>>('/web/traces/spans', {
    params: {
      ...rest,
      ...(filters && filters.length > 0 ? { filters: JSON.stringify(filters) } : {}),
      ...(where ? { where: JSON.stringify(where) } : {}),
    },
  });
  return data;
}

/** A span of another trace that links to a span of this one. */
export interface LinkedSpan {
  traceId: string;
  spanId: string;
  service: string;
  operation: string;
  startNs: number;
  durationNs: number;
  status: string;
  /** The span of this trace the link points at. */
  linkedSpanId: string;
  attributes: Record<string, unknown>;
}

export interface LinkedSpansResponse {
  /** The newest first. */
  spans: LinkedSpan[];
  /** More spans link here than are listed. */
  more: boolean;
  unavailable?: 'time_budget' | 'busy' | 'scan_limit';
}

/** The spans of other traces whose links point into this one, looked for in `window`. */
export async function linkedSpans(traceId: string, window: TraceWindow): Promise<LinkedSpansResponse> {
  const { data } = await http.get<LinkedSpansResponse>(`/web/trace/${encodeURIComponent(traceId)}/linked`, {
    params: { from: Math.floor(window.from), to: Math.ceil(window.to) },
  });
  return data;
}

/** What the time of the requests a trace query matches goes to, the request's own time apart. */
export type TimeCategory = 'own' | 'database' | 'cache' | 'http' | 'rpc' | 'messaging' | 'internal' | 'other';

/** Where a sample of the matching requests spent their time; empty with `unavailable` when it could not be read. */
export interface TimeSpent {
  sampled: number;
  totalTime: number;
  parts: Array<{ category: TimeCategory; micros: number }>;
  unavailable?: string;
}

export async function traceTimeSpent(params: { from: number; to: number; filters: TraceFilter[] }): Promise<TimeSpent> {
  const { filters, ...rest } = params;
  const { data } = await http.get<TimeSpent>('/web/traces/time-spent', {
    params: { ...rest, ...(filters.length > 0 ? { filters: JSON.stringify(filters) } : {}) },
  });
  return data;
}
