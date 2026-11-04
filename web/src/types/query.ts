export type QueryLanguage = 'sql' | 'promql';

export interface QueryRequest {
  orgId: string;
  language: QueryLanguage;
  statement: string;
  timeRange: { start: number; end: number };
  stream?: { name: string; streamType: 'logs' | 'metrics' | 'traces' | 'profiles' };
  /** Rows the answer may hold; a PromQL range answer samples each series evenly to fit. */
  limit?: number;
  /**
   * PromQL range step in microseconds. With it PromQL answers a series per
   * step; without it, the figure at the end of the range. SQL ignores it.
   */
  step?: number;
}

export interface QueryResult {
  columns: string[];
  rows: unknown[][];
  scannedRows: number;
  tookMs: number;
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
  scannedRows?: number;
  returnedRows?: number;
  tookMs?: number;
  timeRangeSecs?: number;
}
