import { type AddressParams, getOrPost, http } from '@/lib/http';
import type { CursorPage } from '@/pagination/cursor';

/**
 * The rows of the RUM explorer, by the plural names the backend gives its streams
 * (`rum_views` is `views`). A session is derived from views on the server.
 */
export type RumType =
  | 'sessions'
  | 'views'
  | 'actions'
  | 'errors'
  | 'resources'
  | 'long_tasks'
  | 'vitals'
  | 'logs';

export type RumFieldType = 'bool' | 'int64' | 'float64' | 'utf8' | 'timestamp' | 'json';

/** What a column is measured in. A duration is stored in the nanoseconds the SDK sent. */
export type RumUnit =
  | 'none'
  | 'ns'
  | 'ms'
  | 'bytes'
  | 'count'
  | 'score'
  | 'percent'
  | 'per_second'
  | 'px';

export interface RumField {
  name: string;
  dataType: RumFieldType;
  unit: RumUnit;
  /** Its values can be counted; a JSON document has none to count. */
  facetable: boolean;
  /** A filter on it does not read every row. */
  indexed: boolean;
}

export interface RumSchema {
  type: RumType;
  stream: string;
  /** The column that names one row. */
  idField: string;
  /** Rows are reported again as the entity changes; the server reads the latest. */
  versioned: boolean;
  defaultColumns: string[];
  fields: RumField[];
}

export interface RumFilter {
  field: string;
  /**
   * `in` takes a JSON list of one to a hundred values as its value; `exists` and `missing` take
   * none.
   */
  op: '=' | '!=' | '>' | '>=' | '<' | '<=' | 'contains' | 'match' | 'match_text' | 'in' | 'not_in' | 'exists' | 'missing';
  value: string;
  quoted?: boolean;
}

/** A row of the explorer: its columns by name, and when it happened. */
export type RumRow = Record<string, unknown> & { timestamp: number };

export interface RumQuery {
  type: RumType;
  from?: number;
  to?: number;
  filters?: RumFilter[];
  freeText?: string[];
}

export type RumSearchParams = RumQuery & {
  /** The columns to answer with; none asked, the defaults of the type. */
  columns?: string[];
  limit?: number;
  cursor?: string;
};

/** The filters and the words to find of a read, as its address says them: JSON lists. */
function lists(params: {
  filters?: RumFilter[] | undefined;
  freeText?: string[] | undefined;
  columns?: string[] | undefined;
}): AddressParams {
  const json = (list: unknown[] | undefined) => (list && list.length > 0 ? JSON.stringify(list) : undefined);

  return {
    filters: json(params.filters),
    freeText: json(params.freeText),
    columns: json(params.columns),
  };
}

export async function schema(type: RumType, signal?: AbortSignal): Promise<RumSchema> {
  const { data } = await http.get<RumSchema>(`/rum/schema/${type}`, { ...(signal && { signal }) });
  return data;
}

export async function search(params: RumSearchParams): Promise<CursorPage<RumRow>> {
  const { filters, freeText: free_text, columns, ...rest } = params;

  return getOrPost<CursorPage<RumRow>>('/rum/search', {
    params: { ...rest, ...lists({ filters, freeText: free_text, columns }) },
    body: params,
  });
}

export interface RumFacetValue {
  value: string;
  count: number;
}

export interface RumFacets {
  field: string;
  /** The commonest first. */
  values: RumFacetValue[];
  unavailable?: 'time_budget' | 'busy';
}

/** The commonest values of `field` across the rows the query matches. */
export async function facets(
  params: RumQuery & { field: string; limit?: number },
  signal?: AbortSignal,
): Promise<RumFacets> {
  const { filters, freeText: free_text, ...rest } = params;

  return getOrPost<RumFacets>(
    '/rum/facets',
    { params: { ...rest, ...lists({ filters, freeText: free_text }) }, body: params },
    { ...(signal && { signal }) },
  );
}

export interface RumKey {
  key: string;
  /** How many sessions had it. */
  sessions: number;
}

export interface RumKeys {
  column: string;
  /** The one most sessions had first. */
  keys: RumKey[];
  unavailable?: 'time_budget' | 'busy';
}

/** The keys found in a JSON column across the rows the query matches: the feature flags evaluated, the timings marked. */
export async function keys(
  params: RumQuery & { column: 'feature_flags' | 'context' | 'view.custom_timings' | 'usr.extra'; limit?: number },
  signal?: AbortSignal,
): Promise<RumKeys> {
  const { filters, freeText: free_text, ...rest } = params;

  return getOrPost<RumKeys>(
    '/rum/keys',
    { params: { ...rest, ...lists({ filters, freeText: free_text }) }, body: params },
    { ...(signal && { signal }) },
  );
}

/** One stretch of the values of a number, and the rows whose value is in it. */
export interface RumDistributionBar {
  from: number;
  to: number;
  rows: number;
}

export interface RumDistribution {
  field: string;
  unit: RumUnit;
  rows: number;
  min: number | null;
  max: number | null;
  p50: number | null;
  p75: number | null;
  p95: number | null;
  p99: number | null;
  /** Of one width from the smallest value to the 99th percentile, the last counting what lies past it. */
  bars: RumDistributionBar[];
  unavailable?: 'time_budget' | 'busy';
}

/** How the values of a number fall across the rows the query matches. */
export async function distribution(params: RumQuery & { field: string; bars?: number }, signal?: AbortSignal): Promise<RumDistribution> {
  const { filters, freeText: free_text, ...rest } = params;

  return getOrPost<RumDistribution>(
    '/rum/distribution',
    { params: { ...rest, ...lists({ filters, freeText: free_text }) }, body: params },
    { ...(signal && { signal }) },
  );
}

export interface RumHistogramBars {
  /** What one bar stands for, in microseconds. */
  bucketTime: number;
  /** Where the first bar begins: at or before the start of the range. */
  originTime: number;
}

export interface RumHistogram extends RumHistogramBars {
  buckets: Array<{ startTime: number; total: number; errors: number }>;
  /** Whether the troubled rows are counted apart from the rest. */
  split: boolean;
  unavailable?: 'time_budget' | 'busy' | 'timestamp_masked';
}

/** How many rows the query matches in each bar of `[from, to)`. */
export async function histogram(
  params: RumQuery & RumHistogramBars & { from: number; to: number },
  signal?: AbortSignal,
): Promise<RumHistogram> {
  const { filters, freeText: free_text, ...rest } = params;

  return getOrPost<RumHistogram>(
    '/rum/histogram',
    { params: { ...rest, ...lists({ filters, freeText: free_text }) }, body: params },
    { ...(signal && { signal }) },
  );
}

/** One row with all its columns. The range is where it happened, when the page knows. */
export async function entity(
  type: RumType,
  id: string,
  range?: { from: number; to: number },
  signal?: AbortSignal,
): Promise<RumRow> {
  const { data } = await http.get<RumRow>(`/rum/${type}/${encodeURIComponent(id)}`, {
    ...(range && { params: range }),
    ...(signal && { signal }),
  });
  return data;
}
