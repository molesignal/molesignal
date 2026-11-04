import type { AxiosResponse } from 'axios';

import { type AddressParams, getOrPost, http } from '@/lib/http';
import type { CursorPage } from '@/pagination/cursor';

export type WebSearchKind = 'stream' | 'service' | 'dashboard' | 'saved_view' | 'alert' | 'incident';

export interface WebSearchItem {
  kind: WebSearchKind;
  id: string;
  label: string;
  subtitle?: string;
}

export interface WebSearchResponse {
  items: WebSearchItem[];
}

export async function search(
  q: string,
  types: WebSearchKind[] = ['stream', 'service', 'dashboard', 'saved_view', 'alert', 'incident'],
  limit = 20,
): Promise<WebSearchResponse> {
  const { data } = await http.get<WebSearchResponse>('/web/search', {
    params: { q, types: types.join(','), limit },
  });
  return data;
}

export interface TopologyNode {
  id: string;
  name: string;
  errorRate: number;
  p95Ms: number;
  rps: number;
  spanCount: number;
}

export interface TopologyEdge {
  source: string;
  target: string;
  rps: number;
  errRate: number;
  p95Ms: number;
}

export interface TopologyResponse {
  nodes: TopologyNode[];
  edges: TopologyEdge[];
}

export async function topology(from: string, to: string): Promise<TopologyResponse> {
  const { data } = await http.get<TopologyResponse>('/web/topology', { params: { from, to } });
  return data;
}

export interface Span {
  spanId: string;
  parentSpanId?: string;
  service: string;
  operation: string;
  startNs: number;
  endNs: number;
  durationNs: number;
  kind: number;
  status: 'OK' | 'ERROR' | 'TIMED_OUT' | string;
  statusMessage?: string;
  traceFlags: number;
  traceState: string;
  resource: {
    attributes: Record<string, unknown>;
    dropped_attributes_count: number;
    schema_url?: string;
  };
  scope: {
    name: string;
    version: string;
    attributes: Record<string, unknown>;
    dropped_attributes_count: number;
    schema_url?: string;
  };
  attributes: Record<string, unknown>;
  events: Array<{
    tsNs: number;
    name: string;
    attributes: Record<string, unknown>;
    droppedAttributesCount: number;
  }>;
  links: Array<{
    trace_id: string;
    span_id: string;
    trace_state: string;
    flags: number;
    attributes: Record<string, unknown>;
    dropped_attributes_count: number;
  }>;
  droppedAttributesCount: number;
  droppedEventsCount: number;
  droppedLinksCount: number;
  schemaVersion: number;
  semanticConventionsVersion: string;
  samplingReason: string;
  partial: boolean;
  partialReasons: string[];
  late: boolean;
  duplicate: boolean;
  conflict: boolean;
}

export interface TraceResponse {
  traceId: string;
  rootSpanId: string;
  spans: Span[];
  truncated?: boolean;
  partial: boolean;
  partialReasons: string[];
  samplingReasons: string[];
  lateSpanCount: number;
  duplicateSpanCount: number;
  conflictSpanCount: number;
}

/** Where a trace is looked for: the moments its spans began in, in epoch microseconds. */
export interface TraceWindow {
  from: number;
  to: number;
}

/**
 * A trace's spans. Without a window the last day is looked in, then the last
 * 31 days for the trace's summary, which says when it ran.
 */
export async function trace(traceId: string, window?: TraceWindow): Promise<TraceResponse> {
  const { data } = await http.get<TraceResponse>(`/web/trace/${encodeURIComponent(traceId)}`, {
    ...(window ? { params: { from: Math.floor(window.from), to: Math.ceil(window.to) } } : {}),
  });
  return data;
}

export interface TraceListItem {
  traceId: string;
  service: string;
  operation: string;
  startNs: number;
  durationMs: number;
  spanCount: number;
  errorCount: number;
  /** The status of the trace's root span: `OK`, `ERROR` or `UNSET`. */
  status?: string;
  /** The HTTP status the root answered with, where it kept one. */
  httpStatusCode?: number;
  /** The fields of the root span that were asked for. */
  fields?: Record<string, unknown>;
}

export type TraceListResponse = CursorPage<TraceListItem>;

export interface TraceFilter {
  field: string;
  op: '=' | '!=' | '>' | '>=' | '<' | '<=' | 'contains' | 'in';
  /** Empty for `in`, which holds for any of `values`. */
  value: string;
  values?: string[];
}

/**
 * What a trace query says beyond its list of filters and its free text: a
 * comparison, text to find, or a group of conditions that all hold (`and`) or
 * one of which holds (`or`). It holds together with the list and the text.
 */
export type TraceCondition =
  | TraceFilter
  | { text: string }
  | { and: TraceCondition[] }
  | { or: TraceCondition[] };

export type TraceListSort =
  | 'latest'
  | 'earliest'
  | 'duration_desc'
  | 'duration_asc'
  | 'span_count_desc'
  | 'errors_desc';

export async function traces(params: {
  from?: number;
  to?: number;
  limit?: number;
  q?: string;
  filters?: TraceFilter[];
  where?: TraceCondition;
  sort?: TraceListSort;
  cursor?: string;
  /** Fields of each trace's root span to answer with. */
  fields?: string[];
} = {}): Promise<TraceListResponse> {
  const { filters, where, fields, ...rest } = params;
  const requestParams = {
    ...rest,
    ...(filters && filters.length > 0 ? { filters: JSON.stringify(filters) } : {}),
    ...(where ? { where: JSON.stringify(where) } : {}),
    ...(fields && fields.length > 0 ? { fields: fields.join(',') } : {}),
  };
  const { data } = await http.get<TraceListResponse>('/web/traces', { params: requestParams });
  return data;
}

/**
 * How many results a list query matches, in the bars a chart asked for. Only
 * the bars that hold something are listed. `unavailable` says why there are no
 * counts, when there are none.
 */
export interface HistogramCountsResponse {
  bucketTime: number;
  originTime: number;
  /** A bar's results, and, where they have a duration, how long they took at three percentiles. */
  buckets: Array<{
    startTime: number;
    total: number;
    errors: number;
    p50Ns?: number;
    p95Ns?: number;
    p99Ns?: number;
  }>;
  /** Whether errors are counted apart from the rest. */
  split: boolean;
  unavailable?: 'time_budget' | 'busy' | 'scan_limit' | 'timestamp_masked';
}

/** The bars to count into: each `bucket_micros` long, the first beginning at `origin_micros`. */
export interface HistogramBars {
  bucketTime: number;
  originTime: number;
}

/** Counts every trace the list query matches in `[from, to)` by when it began, traces with errors apart. */
export async function tracesHistogram(
  params: {
    from: number;
    to: number;
    q?: string;
    filters?: TraceFilter[];
    where?: TraceCondition;
  } & HistogramBars,
  signal?: AbortSignal,
): Promise<HistogramCountsResponse> {
  const { filters, where, ...rest } = params;
  const requestParams = {
    ...rest,
    ...(filters && filters.length > 0 ? { filters: JSON.stringify(filters) } : {}),
    ...(where ? { where: JSON.stringify(where) } : {}),
  };
  const { data } = await http.get<HistogramCountsResponse>('/web/traces/histogram', {
    params: requestParams,
    ...(signal && { signal }),
  });
  return data;
}

export interface LogListFilter {
  field: string;
  op: '=' | '!=' | '>' | '>=' | '<' | '<=' | 'contains' | 'match' | 'match_text';
  value: string;
  quoted?: boolean;
}

/**
 * What a log query says beyond its lists of filters and words to find: a
 * filter, text to find, or a group of conditions that all hold (`and`) or one
 * of which holds (`or`). It holds together with the lists.
 */
export type LogCondition =
  | LogListFilter
  | { text: string }
  | { and: LogCondition[] }
  | { or: LogCondition[] };

export type LogListResponse = CursorPage<Record<string, unknown>>;

/** The filters, the words to find and the condition tree of a log read, as its address says them: JSON. */
function logLists(filters?: LogListFilter[], freeText?: string[], where?: LogCondition): AddressParams {
  return {
    filters: filters && filters.length > 0 ? JSON.stringify(filters) : undefined,
    freeText: freeText && freeText.length > 0 ? JSON.stringify(freeText) : undefined,
    where: where ? JSON.stringify(where) : undefined,
  };
}

export async function logs(params: {
  stream?: string;
  from?: number;
  to?: number;
  filters?: LogListFilter[];
  freeText?: string[];
  where?: LogCondition;
  limit?: number;
  cursor?: string;
}): Promise<LogListResponse> {
  const { filters, freeText: free_text, where, ...rest } = params;
  return getOrPost<LogListResponse>('/web/logs', {
    params: { ...rest, ...logLists(filters, free_text, where) },
    body: params,
  });
}

/** Counts every log row the list query matches in `[from, to)`, rows at level ERROR apart. */
export async function logsHistogram(
  params: {
    stream: string;
    from: number;
    to: number;
    filters?: LogListFilter[];
    freeText?: string[];
    where?: LogCondition;
  } & HistogramBars,
  signal?: AbortSignal,
): Promise<HistogramCountsResponse> {
  const { filters, freeText: free_text, where, ...rest } = params;
  return getOrPost<HistogramCountsResponse>(
    '/web/logs/histogram',
    { params: { ...rest, ...logLists(filters, free_text, where) }, body: params },
    { ...(signal && { signal }) },
  );
}

/**
 * What one field holds across the rows a query matches: how many have a value,
 * how many different values, the commonest of them, and for a number its range,
 * its spread and its values counted into bars. `unavailable` says why there is
 * no analysis, when there is none.
 */
export interface FieldStatsResponse {
  kind: 'number' | 'time' | 'text' | 'bool';
  total: number;
  present: number;
  distinct: number;
  /** Whether `distinct` was counted. Otherwise it is an estimate. */
  distinctExact: boolean;
  /** As text: a count of nanoseconds is past what a JSON number holds exactly. */
  range?: { min: string; max: string };
  spread?: {
    avg: number | null;
    q1: number | null;
    median: number | null;
    q3: number | null;
    stddevSamp: number | null;
    stddevPop: number | null;
    skewness: number | null;
    kurtosis: number | null;
  };
  bars?: { origin: number; width: number; counts: number[] };
  top: Array<{ value: string; count: number }>;
  topSkipped: boolean;
  /** The five commonest values counted over stretches of time; absent where none is commoner than the rest. */
  trend?: {
    originTime: number;
    bucketTime: number;
    series: Array<{ value: string; counts: number[] }>;
  };
  unavailable?: 'time_budget' | 'busy' | 'field_protected';
}

/** Reads `field` in every log row the list query matches in `[from, to)`. */
export async function logsFieldStats(
  params: {
    stream: string;
    from: number;
    to: number;
    filters?: LogListFilter[];
    freeText?: string[];
    where?: LogCondition;
    field: string;
  },
  signal?: AbortSignal,
): Promise<FieldStatsResponse> {
  const { filters, freeText: free_text, where, ...rest } = params;
  return getOrPost<FieldStatsResponse>(
    '/web/logs/field-stats',
    { params: { ...rest, ...logLists(filters, free_text, where) }, body: params },
    { ...(signal && { signal }) },
  );
}

/** Reads a field of a Span in every Span of the listed Traces. */
export async function tracesFieldStats(
  params: {
    stream?: string;
    from: number;
    to: number;
    traceIds: string[];
    field: string;
  },
  signal?: AbortSignal,
): Promise<FieldStatsResponse> {
  const { data } = await http.post<FieldStatsResponse>('/web/traces/field-stats', params, {
    ...(signal && { signal }),
  });
  return data;
}

export interface Filter {
  field: string;
  op: '=' | '!=' | 'IN' | 'CONTAINS';
  value: string | string[];
}

export interface CorrelationContext {
  timeRange: { from: string; to: string };
  filters: Filter[];
  prefill?: { sql?: string; promql?: string };
}

export async function correlation(
  from: string,
  to: string,
  ctx: Record<string, unknown>,
  signal?: AbortSignal,
): Promise<CorrelationContext> {
  // Backend decodes with `URL_SAFE_NO_PAD` — convert standard base64 to that
  // alphabet: `+` → `-`, `/` → `_`, strip `=` padding.
  const b64 = btoa(JSON.stringify(ctx))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
  const { data } = await http.get<CorrelationContext, AxiosResponse<CorrelationContext>>(
    `/web/correlation/${from}/${to}`,
    {
      params: { ctx: b64 },
      ...(signal && { signal }),
    },
  );
  return data;
}

export interface BlobRef {
  blobId: string;
}

export async function storeInvestigationBlob(payload: Record<string, unknown>): Promise<BlobRef> {
  const { data } = await http.post<BlobRef>('/web/investigation/blob', payload);
  return data;
}

export async function fetchInvestigationBlob(id: string): Promise<Record<string, unknown>> {
  const { data } = await http.get<Record<string, unknown>>(`/web/investigation/blob/${id}`);
  return data;
}
