import { http } from '@/lib/http';
import type { QueryProfile, QueryRecommendation, QueryRequest, QueryResult } from '@/types/query';

export type PromqlCapabilityKind = 'function' | 'aggregation' | 'keyword' | 'operator';

export interface PromqlCapabilityItem {
  label: string;
  insert_text: string;
  detail: string;
  documentation: string;
  kind: PromqlCapabilityKind;
}

export interface PromqlCapabilities {
  engine: string;
  version: number;
  functions: PromqlCapabilityItem[];
  aggregations: PromqlCapabilityItem[];
  keywords: PromqlCapabilityItem[];
  operators: PromqlCapabilityItem[];
}

export async function runQuery(req: QueryRequest): Promise<QueryResult> {
  const { data } = await http.post<QueryResult>('/query', req, { headers: { Prefer: 'respond-sync' } });
  return data;
}

export interface PrometheusExemplar {
  labels: Record<string, string>;
  value: number;
  /** Prometheus HTTP API timestamp in epoch seconds. */
  timestamp: number;
}

export interface PrometheusExemplarSeries {
  seriesLabels: Record<string, string>;
  exemplars: PrometheusExemplar[];
}

export interface PrometheusExemplarResponse {
  status: 'success';
  data: PrometheusExemplarSeries[];
  warnings?: string[];
}

export async function fetchPrometheusExemplars(params: {
  query: string;
  startMicros: number;
  endMicros: number;
}): Promise<PrometheusExemplarResponse> {
  const { data } = await http.get<PrometheusExemplarResponse>(
    '/prometheus/api/v1/query_exemplars',
    {
      params: {
        query: params.query,
        start: params.startMicros / 1_000_000,
        end: params.endMicros / 1_000_000,
      },
    },
  );
  return data;
}

export async function fetchPromqlCapabilities(): Promise<PromqlCapabilities> {
  const { data } = await http.get<PromqlCapabilities>('/query/promql/capabilities');
  return data;
}

/**
 * Advisory query-optimization tips for a query profile. Stateless — it does
 * not re-run the query; pass the stats (`scanned_rows` / `took_ms` / row
 * count / time range) from the previous query response.
 */
export async function recommendations(
  profile: QueryProfile,
): Promise<{ recommendations: QueryRecommendation[] }> {
  const { data } = await http.post<{ recommendations: QueryRecommendation[] }>(
    '/query/recommendations',
    profile,
  );
  return data;
}

/**
 * Build the absolute URL for `GET /api/v1/query/stream`. The endpoint emits
 * an NDJSON stream (one row per line, terminated by a `__meta__` line); the
 * LogStream view consumes it directly via fetch + ReadableStream rather than
 * axios because axios buffers the whole response.
 */
export function streamQueryUrl(params: {
  sql: string;
  stream: string;
  stream_type?: 'logs' | 'metrics' | 'traces';
  from: number;
  to: number;
  limit?: number;
}): string {
  const search = new URLSearchParams();
  search.set('sql', params.sql);
  search.set('stream', params.stream);
  if (params.stream_type) search.set('stream_type', params.stream_type);
  search.set('from', String(params.from));
  search.set('to', String(params.to));
  if (params.limit !== undefined) search.set('limit', String(params.limit));
  return `/api/v1/query/stream?${search.toString()}`;
}
