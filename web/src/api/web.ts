import type { AxiosResponse } from 'axios';

import { http } from '@/lib/http';
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
  error_rate: number;
  p95_ms: number;
  rps: number;
  span_count: number;
}

export interface TopologyEdge {
  source: string;
  target: string;
  rps: number;
  err_rate: number;
  p95_ms: number;
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
  span_id: string;
  parent_span_id?: string;
  service: string;
  operation: string;
  start_ns: number;
  end_ns: number;
  duration_ns: number;
  kind: number;
  status: 'OK' | 'ERROR' | 'TIMED_OUT' | string;
  status_message?: string;
  trace_flags: number;
  trace_state: string;
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
    ts_ns: number;
    name: string;
    attributes: Record<string, unknown>;
    dropped_attributes_count: number;
  }>;
  links: Array<{
    trace_id: string;
    span_id: string;
    trace_state: string;
    flags: number;
    attributes: Record<string, unknown>;
    dropped_attributes_count: number;
  }>;
  dropped_attributes_count: number;
  dropped_events_count: number;
  dropped_links_count: number;
  schema_version: number;
  semantic_conventions_version: string;
  sampling_reason: string;
  partial: boolean;
  partial_reasons: string[];
  late: boolean;
  duplicate: boolean;
  conflict: boolean;
}

export interface TraceResponse {
  trace_id: string;
  root_span_id: string;
  spans: Span[];
  truncated?: boolean;
  partial: boolean;
  partial_reasons: string[];
  sampling_reasons: string[];
  late_span_count: number;
  duplicate_span_count: number;
  conflict_span_count: number;
}

export async function trace(traceId: string): Promise<TraceResponse> {
  const { data } = await http.get<TraceResponse>(`/web/trace/${encodeURIComponent(traceId)}`);
  return data;
}

export interface TraceListItem {
  trace_id: string;
  service: string;
  operation: string;
  start_ns: number;
  duration_ms: number;
  span_count: number;
  error_count: number;
}

export type TraceListResponse = CursorPage<TraceListItem>;

export interface TraceFilter {
  field: string;
  op: '=' | '!=' | '>' | '>=' | '<' | '<=' | 'contains';
  value: string;
}

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
  sort?: TraceListSort;
  cursor?: string;
} = {}): Promise<TraceListResponse> {
  const { filters, ...rest } = params;
  const requestParams = {
    ...rest,
    ...(filters && filters.length > 0 ? { filters: JSON.stringify(filters) } : {}),
  };
  const { data } = await http.get<TraceListResponse>('/web/traces', { params: requestParams });
  return data;
}

export interface LogListFilter {
  field: string;
  op: '=' | '!=' | '>' | '>=' | '<' | '<=' | 'contains' | 'match' | 'match_text';
  value: string;
  quoted?: boolean;
}

export type LogListResponse = CursorPage<Record<string, unknown>>;

export async function logs(params: {
  stream?: string;
  from?: number;
  to?: number;
  filters?: LogListFilter[];
  free_text?: string[];
  limit?: number;
  cursor?: string;
}): Promise<LogListResponse> {
  const { data } = await http.post<LogListResponse>('/web/logs', params);
  return data;
}

export interface Filter {
  field: string;
  op: '=' | '!=' | 'IN' | 'CONTAINS';
  value: string | string[];
}

export interface CorrelationContext {
  time_range: { from: string; to: string };
  filters: Filter[];
  prefill?: { sql?: string; promql?: string };
}

export interface CorrelationProvider {
  id: string;
  from_kind: string;
  to_kind: string;
  label: string;
  enabled: boolean;
}

export async function correlationProviders(): Promise<CorrelationProvider[]> {
  const { data } = await http.get<CorrelationProvider[]>('/web/correlation/providers');
  return data;
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
  blob_id: string;
}

export async function storeInvestigationBlob(payload: Record<string, unknown>): Promise<BlobRef> {
  const { data } = await http.post<BlobRef>('/web/investigation/blob', payload);
  return data;
}

export async function fetchInvestigationBlob(id: string): Promise<Record<string, unknown>> {
  const { data } = await http.get<Record<string, unknown>>(`/web/investigation/blob/${id}`);
  return data;
}
