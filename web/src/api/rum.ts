import { http } from '@/lib/http';
import type { CursorPage } from '@/pagination/cursor';

/**
 * RUM client. List, overview, detail, and performance pages use dedicated
 * read-model endpoints backed by narrow physical Parquet projections. Replay
 * segments use `/rum/replay/:session_id` because their payload lives in object
 * storage rather than the RUM event read models.
 */

export type ExperienceGrade = 'good' | 'needs_improvement' | 'poor' | 'unknown';

export interface SessionRow {
  session_id: string;
  user_id?: string;
  ip_address?: string;
  country?: string;
  browser?: string;
  application?: string;
  environment?: string;
  version?: string;
  device?: string;
  os?: string;
  duration_ms?: number;
  error_count?: number;
  started_at_micros?: number;
  landing_page?: string;
  last_page?: string;
  journey: string[];
  rage_click_count: number;
  dead_click_count: number;
  slow_resource_count: number;
  failed_request_count: number;
  crash_count: number;
  lcp_ms?: number;
  fid_ms?: number;
  inp_ms?: number;
  cls?: number;
  ttfb_ms?: number;
  experience: ExperienceGrade;
  replay_available: boolean;
}

export interface SessionEvent {
  ts_micros: number;
  type: string;
  name?: string;
  url?: string;
  duration_ms?: number;
  status?: number;
  payload: Record<string, unknown>;
  /** Phase 6+ M2 cross-signal handles. RUM SDK writes these from the W3C
   *  Trace Context (`traceparent` header) of the fetch/XHR that produced
   *  the action. May be missing on old SDK versions — UI degrades gracefully. */
  service?: string;
  trace_id?: string;
  parent_span_id?: string;
}

export interface RelatedTraceRow {
  trace_id: string;
  service?: string;
  span_count: number;
  duration_ms?: number;
  started_at_micros?: number;
  /** `direct` = the session has an action whose `trace_id` column matches.
   *  `time-correlated` = no direct trace was found, so the backend widened
   *  the search to traces whose start falls inside the session window. */
  relation: 'direct' | 'time-correlated';
}

export interface RelatedTraces {
  session_id: string;
  primary_service?: string;
  traces: RelatedTraceRow[];
}

export interface ErrorRow {
  fingerprint: string;
  message: string;
  count: number;
  users: number;
  sessions: number;
  first_seen_micros: number;
  last_seen_micros: number;
  page?: string;
  version?: string;
  error_type?: string;
  trend_pct: number;
  status: 'new' | 'ongoing';
  recent_sessions: string[];
  recent_users: string[];
}

export interface ErrorDetail {
  fingerprint: string;
  message: string;
  stack: ErrorStackFrame[];
  recent_sessions: string[];
  count: number;
  users: number;
  first_seen_micros: number;
  last_seen_micros: number;
  pages: string[];
  versions: string[];
}

export interface ErrorStackFrame {
  file?: string;
  function?: string;
  line?: number;
  column?: number;
  original_file?: string;
  original_function?: string;
  original_line?: number;
  original_column?: number;
}

export interface WebVitalsPoint {
  ts_micros: number;
  session_id?: string;
  page?: string;
  application?: string;
  environment?: string;
  version?: string;
  browser?: string;
  country?: string;
  device?: string;
  lcp_ms?: number;
  fid_ms?: number;
  inp_ms?: number;
  cls?: number;
  ttfb_ms?: number;
}

export interface ReplayEvent {
  type: string | number;
  timestamp?: number;
  ts?: number;
  [key: string]: unknown;
}

export interface SessionReplay {
  session_id: string;
  segment_count: number;
  events: ReplayEvent[];
}

export interface ApiPerfRow {
  url: string;
  count: number;
  p50_ms: number;
  p95_ms: number;
  err_rate: number;
}

export interface TimeRangeQueryParams {
  org_id: string;
  from_micros: number;
  to_micros: number;
  limit?: number;
}

export interface SessionListParams extends TimeRangeQueryParams {
  q?: string;
  country?: string;
  browser?: string;
  replay_available?: boolean;
  cursor?: string;
}

export interface ErrorListParams extends TimeRangeQueryParams {
  q?: string;
  status?: 'new' | 'ongoing';
  cursor?: string;
}

function num(value: unknown): number | undefined {
  if (typeof value === 'number') return value;
  if (typeof value === 'string') {
    const n = Number(value);
    return Number.isFinite(n) ? n : undefined;
  }
  return undefined;
}

function str(value: unknown): string | undefined {
  if (typeof value === 'string') return value;
  if (value == null) return undefined;
  return String(value);
}

function bool(value: unknown): boolean {
  return value === true || value === 'true' || value === 1 || value === '1';
}

function firstString(...values: unknown[]): string | undefined {
  for (const value of values) {
    const parsed = str(value);
    if (parsed && parsed.trim()) return parsed;
  }
  return undefined;
}

function parseSessionRow(raw: Record<string, unknown>): SessionRow {
  const row: SessionRow = {
    session_id: str(raw.session_id) ?? '',
    journey: [],
    rage_click_count: 0,
    dead_click_count: 0,
    slow_resource_count: 0,
    failed_request_count: 0,
    crash_count: 0,
    experience: 'unknown',
    replay_available: bool(raw.replay_available),
  };
  const stringFields = [
    'user_id',
    'country',
    'browser',
    'application',
    'environment',
    'version',
    'device',
    'os',
    'landing_page',
    'last_page',
  ] as const;
  for (const field of stringFields) {
    const value = str(raw[field]);
    if (value !== undefined) row[field] = value;
  }
  const ipAddress = firstString(raw.ip_address, raw.client_ip, raw.ip);
  if (ipAddress !== undefined) row.ip_address = ipAddress;
  const numericFields = [
    'duration_ms',
    'error_count',
    'started_at_micros',
    'lcp_ms',
    'fid_ms',
    'inp_ms',
    'cls',
    'ttfb_ms',
  ] as const;
  for (const field of numericFields) {
    const value = num(raw[field]);
    if (value !== undefined) row[field] = value;
  }
  const journey = Array.isArray(raw.journey)
    ? raw.journey.map(str).filter((value): value is string => Boolean(value))
    : [];
  row.journey = journey;
  for (const field of [
    'rage_click_count',
    'dead_click_count',
    'slow_resource_count',
    'failed_request_count',
    'crash_count',
  ] as const) {
    row[field] = num(raw[field]) ?? 0;
  }
  const experience = str(raw.experience);
  if (
    experience === 'good' ||
    experience === 'needs_improvement' ||
    experience === 'poor' ||
    experience === 'unknown'
  ) {
    row.experience = experience;
  }
  return row;
}

/* ─────────── Sessions ─────────── */

export async function listSessions(
  params: SessionListParams,
): Promise<CursorPage<SessionRow>> {
  const limit = params.limit ?? 20;
  const { data: sessionsPage } = await http.get<
    CursorPage<Record<string, unknown>>
  >('/rum/sessions', {
    params: {
      from: params.from_micros,
      to: params.to_micros,
      limit,
      ...(params.q ? { q: params.q } : {}),
      ...(params.country ? { country: params.country } : {}),
      ...(params.browser ? { browser: params.browser } : {}),
      ...(params.replay_available ? { replay_available: true } : {}),
      ...(params.cursor ? { cursor: params.cursor } : {}),
    },
  });
  return {
    ...sessionsPage,
    items: sessionsPage.items
      .map(parseSessionRow)
      .filter((row) => row.session_id.length > 0),
  };
}

export async function getSession(params: {
  org_id: string;
  session_id: string;
  from_micros: number;
  to_micros: number;
}): Promise<{ session: SessionRow | null; events: SessionEvent[] }> {
  const { data } = await http.get<{
    session: Record<string, unknown> | null;
    events: SessionEvent[];
  }>(`/rum/sessions/${encodeURIComponent(params.session_id)}`, {
    params: {
      from: params.from_micros,
      to: params.to_micros,
    },
  });
  return {
    session: data.session ? parseSessionRow(data.session) : null,
    events: data.events,
  };
}

/**
 * Fetches the backend traces correlated with a RUM session. See
 * `src/api/http/routes/rum/query.rs` — the backend tries the direct
 * path first (trace IDs from the session's action read model are resolved via
 * the trace summary reader) and falls back to time-correlation against the
 * session's `started_at + duration` window when actions have no trace ID.
 *
 * Used by `SessionDetail` to populate the "Related traces" panel. Errors
 * are surfaced via React Query; a 404 / empty list means the session has
 * no backend correlation and the panel renders an empty state.
 */
export async function relatedTraces(sessionId: string): Promise<RelatedTraces> {
  const { data } = await http.get<RelatedTraces>(
    `/rum/sessions/${encodeURIComponent(sessionId)}/related-traces`,
  );
  return data;
}

export async function getReplay(sessionId: string): Promise<SessionReplay> {
  const { data } = await http.get<SessionReplay>(
    `/rum/replay/${encodeURIComponent(sessionId)}`,
  );
  return data;
}

/* ─────────── Errors ─────────── */

export async function listErrors(
  params: ErrorListParams,
): Promise<CursorPage<ErrorRow>> {
  const { data } = await http.get<CursorPage<ErrorRow>>('/rum/errors', {
    params: {
      from: params.from_micros,
      to: params.to_micros,
      limit: params.limit ?? 20,
      ...(params.q ? { q: params.q } : {}),
      ...(params.status ? { status: params.status } : {}),
      ...(params.cursor ? { cursor: params.cursor } : {}),
    },
  });
  return data;
}

export async function getError(params: {
  org_id: string;
  fingerprint: string;
  from_micros: number;
  to_micros: number;
}): Promise<ErrorDetail | null> {
  const { data } = await http.get<ErrorDetail | null>(
    `/rum/errors/${encodeURIComponent(params.fingerprint)}`,
    {
      params: {
        from: params.from_micros,
        to: params.to_micros,
      },
    },
  );
  return data;
}

/* ─────────── Performance ─────────── */

export async function webVitalsSeries(params: TimeRangeQueryParams): Promise<WebVitalsPoint[]> {
  const limit = params.limit ?? 200;
  const { data } = await http.get<WebVitalsPoint[]>('/rum/performance/vitals', {
    params: {
      from: params.from_micros,
      to: params.to_micros,
      limit,
    },
  });
  return data;
}

export async function apiPerformance(params: TimeRangeQueryParams): Promise<ApiPerfRow[]> {
  const limit = params.limit ?? 50;
  const { data } = await http.get<ApiPerfRow[]>('/rum/performance/apis', {
    params: {
      from: params.from_micros,
      to: params.to_micros,
      limit,
    },
  });
  return data;
}

export async function errorRateSeries(params: TimeRangeQueryParams): Promise<Array<{ ts_micros: number; count: number }>> {
  const limit = params.limit ?? 200;
  const { data } = await http.get<Array<{ ts_micros: number; count: number }>>(
    '/rum/performance/errors',
    {
      params: {
        from: params.from_micros,
        to: params.to_micros,
        limit,
      },
    },
  );
  return data;
}
