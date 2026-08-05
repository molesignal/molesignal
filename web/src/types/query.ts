export type QueryLanguage = 'sql' | 'promql';

export interface QueryRequest {
  org_id: string;
  language: QueryLanguage;
  statement: string;
  time_range: { start: number; end: number };
  stream?: { name: string; stream_type: 'logs' | 'metrics' | 'traces' };
  limit?: number;
}

export interface QueryResult {
  columns: string[];
  rows: unknown[][];
  scanned_rows: number;
  took_ms: number;
}

export type RecommendationSeverity = 'info' | 'warning' | 'critical';

/** One advisory optimization tip for a query (from `POST /query/recommendations`). */
export interface QueryRecommendation {
  /** Stable identifier (e.g. `slow_query`); usable for i18n / dedupe. */
  code: string;
  severity: RecommendationSeverity;
  title: string;
  detail: string;
}

/** Query profile sent to the recommendations endpoint. Stats come from the
 * previous query response (`scanned_rows` / `took_ms` / row count). */
export interface QueryProfile {
  language: QueryLanguage;
  statement: string;
  scanned_rows?: number;
  returned_rows?: number;
  took_ms?: number;
  time_range_secs?: number;
}
