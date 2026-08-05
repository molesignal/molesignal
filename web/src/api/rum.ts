import { http, toApiError } from '@/lib/http';
import type { CursorPage } from '@/pagination/cursor';
import type { QueryRequest, QueryResult } from '@/types/query';

/**
 * RUM client. Session, action, error, and vital data is queried from the
 * underlying Logs streams (`rum_sessions`, `rum_actions`, `rum_errors`)
 * through `POST /query`. Replay segments are read through the dedicated
 * `/rum/replay/:session_id` endpoint because their payload lives in
 * object storage rather than a queryable stream.
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

const STREAM_TYPE = 'logs' as const;

async function runRumQuery(req: QueryRequest): Promise<QueryResult> {
  try {
    const { data } = await http.post<QueryResult>('/query', req, {
      headers: { Prefer: 'respond-sync' },
    });
    return data;
  } catch (err) {
    // 空实例：RUM stream（rum_sessions / rum_actions / rum_errors）由首次 ingest 才按需建出
    // （schema-on-write）；未建时 query planner 返 forbidden("stream not found: …")。视作空
    // 结果，让 RUM 页面渲染空态而非报错 - RUM 流名是固定隐式的，没有数据≠查询出错。
    if (toApiError(err).message.includes('stream not found')) {
      return { columns: [], rows: [], scanned_rows: 0, took_ms: 0 };
    }
    throw err;
  }
}

function toRecord(result: QueryResult): Array<Record<string, unknown>> {
  return result.rows.map((row) => {
    const rec: Record<string, unknown> = {};
    result.columns.forEach((col, i) => {
      rec[col] = row[i];
    });
    return rec;
  });
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

function record(value: unknown): Record<string, unknown> {
  if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  if (typeof value === 'string') {
    try {
      const parsed = JSON.parse(value);
      if (typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) {
        return parsed as Record<string, unknown>;
      }
    } catch {
      return {};
    }
  }
  return {};
}

function firstString(...values: unknown[]): string | undefined {
  for (const value of values) {
    const parsed = str(value);
    if (parsed && parsed.trim()) return parsed;
  }
  return undefined;
}

function assignString(
  target: Record<string, unknown>,
  key: string,
  ...values: unknown[]
): void {
  const value = firstString(...values);
  if (value !== undefined) target[key] = value;
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
  return row;
}

interface SessionActionSummary {
  hasActions: boolean;
  journey: string[];
  rageClicks: number;
  deadClicks: number;
  slowResources: number;
  failedRequests: number;
  crashes: number;
  lcp?: number;
  fid?: number;
  inp?: number;
  cls?: number;
  ttfb?: number;
  application?: string;
  environment?: string;
  version?: string;
  device?: string;
  os?: string;
  country?: string;
  browser?: string;
}

function emptyActionSummary(): SessionActionSummary {
  return {
    hasActions: false,
    journey: [],
    rageClicks: 0,
    deadClicks: 0,
    slowResources: 0,
    failedRequests: 0,
    crashes: 0,
  };
}

function actionPage(raw: Record<string, unknown>, payload: Record<string, unknown>): string | undefined {
  const candidate = firstString(
    raw.page,
    payload.path,
    payload.page,
    payload.href,
    raw.url,
  );
  if (!candidate) return undefined;
  try {
    const url = new URL(candidate, 'https://rum.local');
    return `${url.pathname}${url.search === '?' ? '' : url.search}`;
  } catch {
    return candidate;
  }
}

function classifyExperience(session: SessionRow, summary: SessionActionSummary): ExperienceGrade {
  const hasPoorVital =
    (summary.lcp ?? 0) > 4_000 ||
    (summary.fid ?? 0) > 300 ||
    (summary.inp ?? 0) > 500 ||
    (summary.cls ?? 0) > 0.25 ||
    (summary.ttfb ?? 0) > 1_800;
  const hasNeedsVital =
    (summary.lcp ?? 0) > 2_500 ||
    (summary.fid ?? 0) > 100 ||
    (summary.inp ?? 0) > 200 ||
    (summary.cls ?? 0) > 0.1 ||
    (summary.ttfb ?? 0) > 800;
  if (
    (session.error_count ?? 0) > 0 ||
    summary.failedRequests > 0 ||
    summary.rageClicks > 0 ||
    summary.deadClicks > 0 ||
    summary.crashes > 0 ||
    hasPoorVital
  ) {
    return 'poor';
  }
  if (summary.slowResources > 0 || hasNeedsVital) return 'needs_improvement';
  return summary.hasActions ? 'good' : 'unknown';
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
  const sessionIds = sessionsPage.items
    .map((row) => str(row.session_id))
    .filter((value): value is string => Boolean(value));
  const actionLimit = Math.max(500, limit * 20);
  const actionsResult =
    sessionIds.length === 0
      ? { columns: [], rows: [], scanned_rows: 0, took_ms: 0 }
      : await runRumQuery({
          org_id: params.org_id,
          language: 'sql',
          statement: `SELECT * FROM rum_actions WHERE session_id IN (${sessionIds
            .map((value) => `'${value.replace(/'/g, "''")}'`)
            .join(', ')}) ORDER BY ts_micros ASC LIMIT ${actionLimit}`,
          time_range: { start: params.from_micros, end: params.to_micros },
          stream: { name: 'rum_actions', stream_type: STREAM_TYPE },
          limit: actionLimit,
        });
  const summaries = new Map<string, SessionActionSummary>();
  for (const raw of toRecord(actionsResult)) {
    const sessionId = str(raw.session_id);
    if (!sessionId) continue;
    const summary = summaries.get(sessionId) ?? emptyActionSummary();
    summary.hasActions = true;
    const payload = record(raw.payload);
    const type = (str(raw.type) ?? '').toLowerCase().replace(/[-\s]/g, '_');
    const page = actionPage(raw, payload);
    if (page && (type === 'view' || summary.journey.length === 0)) {
      if (summary.journey[summary.journey.length - 1] !== page) summary.journey.push(page);
    }
    if (type === 'rage_click' || type === 'rageclick') summary.rageClicks += 1;
    if (type === 'dead_click' || type === 'deadclick') summary.deadClicks += 1;
    if (type === 'crash') summary.crashes += 1;
    const status = num(raw.status);
    if ((status ?? 0) >= 400 || type === 'error' || type === 'network_error') {
      summary.failedRequests += 1;
    }
    if (type === 'resource' && (num(raw.duration_ms) ?? 0) >= 1_000) {
      summary.slowResources += 1;
    }
    const lcp = num(raw.lcp_ms);
    const fid = num(raw.fid_ms);
    const inp = num(raw.inp_ms ?? payload.inp_ms);
    const cls = num(raw.cls);
    const ttfb = num(raw.ttfb_ms);
    if (lcp !== undefined) summary.lcp = Math.max(summary.lcp ?? 0, lcp);
    if (fid !== undefined) summary.fid = Math.max(summary.fid ?? 0, fid);
    if (inp !== undefined) summary.inp = Math.max(summary.inp ?? 0, inp);
    if (cls !== undefined) summary.cls = Math.max(summary.cls ?? 0, cls);
    if (ttfb !== undefined) summary.ttfb = Math.max(summary.ttfb ?? 0, ttfb);
    assignString(summary as unknown as Record<string, unknown>, 'application', raw.application, payload.application, payload.app);
    assignString(summary as unknown as Record<string, unknown>, 'environment', raw.environment, payload.environment, payload.env);
    assignString(summary as unknown as Record<string, unknown>, 'version', raw.version, raw.release, payload.version, payload.release);
    assignString(summary as unknown as Record<string, unknown>, 'device', raw.device, payload.device);
    assignString(summary as unknown as Record<string, unknown>, 'os', raw.os, payload.os);
    assignString(summary as unknown as Record<string, unknown>, 'country', raw.country, payload.country);
    assignString(summary as unknown as Record<string, unknown>, 'browser', raw.browser, payload.browser);
    summaries.set(sessionId, summary);
  }

  const items = sessionsPage.items
    .map(parseSessionRow)
    .filter((row) => row.session_id.length > 0)
    .map((session) => {
      const summary = summaries.get(session.session_id) ?? emptyActionSummary();
      session.journey = summary.journey;
      const landingPage = session.landing_page ?? summary.journey[0];
      const lastPage = session.last_page ?? summary.journey[summary.journey.length - 1];
      if (landingPage !== undefined) session.landing_page = landingPage;
      if (lastPage !== undefined) session.last_page = lastPage;
      session.rage_click_count = summary.rageClicks;
      session.dead_click_count = summary.deadClicks;
      session.slow_resource_count = summary.slowResources;
      session.failed_request_count = summary.failedRequests;
      session.crash_count = summary.crashes;
      if (session.application === undefined && summary.application !== undefined) {
        session.application = summary.application;
      }
      if (session.environment === undefined && summary.environment !== undefined) {
        session.environment = summary.environment;
      }
      if (session.version === undefined && summary.version !== undefined) {
        session.version = summary.version;
      }
      if (session.device === undefined && summary.device !== undefined) {
        session.device = summary.device;
      }
      if (session.os === undefined && summary.os !== undefined) session.os = summary.os;
      if (session.country === undefined && summary.country !== undefined) {
        session.country = summary.country;
      }
      if (session.browser === undefined && summary.browser !== undefined) {
        session.browser = summary.browser;
      }
      if (summary.lcp !== undefined) session.lcp_ms = summary.lcp;
      if (summary.fid !== undefined) session.fid_ms = summary.fid;
      if (summary.inp !== undefined) session.inp_ms = summary.inp;
      if (summary.cls !== undefined) session.cls = summary.cls;
      if (summary.ttfb !== undefined) session.ttfb_ms = summary.ttfb;
      session.experience = classifyExperience(session, summary);
      return session;
    });
  return { ...sessionsPage, items };
}

export async function getSession(params: {
  org_id: string;
  session_id: string;
  from_micros: number;
  to_micros: number;
}): Promise<{ session: SessionRow | null; events: SessionEvent[] }> {
  const escaped = params.session_id.replace(/'/g, "''");
  const [sessionsResult, actionsResult] = await Promise.all([
    runRumQuery({
      org_id: params.org_id,
      language: 'sql',
      statement: `SELECT * FROM rum_sessions WHERE session_id = '${escaped}' LIMIT 1`,
      time_range: { start: params.from_micros, end: params.to_micros },
      stream: { name: 'rum_sessions', stream_type: STREAM_TYPE },
      limit: 1,
    }),
    runRumQuery({
      org_id: params.org_id,
      language: 'sql',
      statement: `SELECT * FROM rum_actions WHERE session_id = '${escaped}' ORDER BY ts_micros ASC LIMIT 500`,
      time_range: { start: params.from_micros, end: params.to_micros },
      stream: { name: 'rum_actions', stream_type: STREAM_TYPE },
      limit: 500,
    }),
  ]);
  const sessions = toRecord(sessionsResult);
  const first = sessions[0];
  const session = first ? parseSessionRow(first) : null;
  const events: SessionEvent[] = toRecord(actionsResult).map((r) => {
    const evt: SessionEvent = {
      ts_micros: num(r.ts_micros) ?? 0,
      type: str(r.type) ?? 'unknown',
      payload: (r.payload as Record<string, unknown>) ?? {},
    };
    const name = str(r.name);
    if (name !== undefined) evt.name = name;
    const url = actionPage(r, record(r.payload));
    if (url !== undefined) evt.url = url;
    const durationMs = num(r.duration_ms);
    if (durationMs !== undefined) evt.duration_ms = durationMs;
    const status = num(r.status);
    if (status !== undefined) evt.status = status;
    const service = str(r.service);
    if (service !== undefined) evt.service = service;
    const traceId = str(r.trace_id);
    if (traceId !== undefined) evt.trace_id = traceId;
    const parentSpan = str(r.parent_span_id);
    if (parentSpan !== undefined) evt.parent_span_id = parentSpan;
    return evt;
  });
  if (session) {
    const summary = emptyActionSummary();
    for (const event of events) {
      summary.hasActions = true;
      if (event.url && event.type.toLowerCase() === 'view') {
        if (summary.journey[summary.journey.length - 1] !== event.url) summary.journey.push(event.url);
      }
      const type = event.type.toLowerCase().replace(/[-\s]/g, '_');
      if (type === 'rage_click' || type === 'rageclick') summary.rageClicks += 1;
      if (type === 'dead_click' || type === 'deadclick') summary.deadClicks += 1;
      if (type === 'crash') summary.crashes += 1;
      if ((event.status ?? 0) >= 400 || type === 'error' || type === 'network_error') {
        summary.failedRequests += 1;
      }
      if (type === 'resource' && (event.duration_ms ?? 0) >= 1_000) summary.slowResources += 1;
    }
    session.journey = summary.journey;
    const landingPage = session.landing_page ?? summary.journey[0];
    const lastPage = session.last_page ?? summary.journey[summary.journey.length - 1];
    if (landingPage !== undefined) session.landing_page = landingPage;
    if (lastPage !== undefined) session.last_page = lastPage;
    session.rage_click_count = summary.rageClicks;
    session.dead_click_count = summary.deadClicks;
    session.slow_resource_count = summary.slowResources;
    session.failed_request_count = summary.failedRequests;
    session.crash_count = summary.crashes;
    session.experience = classifyExperience(session, summary);
  }
  return { session, events };
}

/**
 * Fetches the backend traces correlated with a RUM session. See
 * `src/api/http/routes/rum/query.rs` — the backend tries the direct
 * path first (sessions's `rum_actions.trace_id` distinct set joined to the
 * traces stream) and falls back to time-correlation against the session's
 * `started_at + duration` window when the actions stream has no trace_id.
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
  const escaped = params.fingerprint.replace(/'/g, "''");
  const result = await runRumQuery({
    org_id: params.org_id,
    language: 'sql',
    statement: `SELECT * FROM rum_errors WHERE fingerprint = '${escaped}' ORDER BY timestamp DESC LIMIT 50`,
    time_range: { start: params.from_micros, end: params.to_micros },
    stream: { name: 'rum_errors', stream_type: STREAM_TYPE },
    limit: 50,
  });
  const rows = toRecord(result);
  const first = rows[0];
  if (!first) return null;
  const errorRaw = first.error;
  let stackRaw: unknown;
  if (typeof errorRaw === 'string') {
    try {
      const parsed = JSON.parse(errorRaw) as { stack?: unknown };
      stackRaw = parsed.stack;
    } catch {
      stackRaw = undefined;
    }
  } else if (typeof errorRaw === 'object' && errorRaw !== null && 'stack' in errorRaw) {
    stackRaw = (errorRaw as { stack?: unknown }).stack;
  }
  let stack: ErrorStackFrame[] = [];
  if (Array.isArray(stackRaw)) {
    stack = stackRaw.map((f) => f as ErrorStackFrame);
  } else if (typeof stackRaw === 'string') {
    try {
      const parsed = JSON.parse(stackRaw);
      if (Array.isArray(parsed)) stack = parsed as ErrorStackFrame[];
    } catch {
      stack = [];
    }
  }
  const sessions = Array.from(
    new Set(rows.map((r) => str(r.session_id)).filter((v): v is string => !!v)),
  ).slice(0, 10);
  const users = new Set(rows.map((r) => str(r.user_id)).filter((v): v is string => !!v));
  const timestamps = rows
    .map((r) => num(r.timestamp))
    .filter((v): v is number => v !== undefined)
    .sort((a, b) => a - b);
  const pages = Array.from(
    new Set(
      rows
        .map((r) => {
          const error = record(r.error);
          return firstString(r.page, r.url, error.page, error.url, record(error.context).page);
        })
        .filter((v): v is string => !!v),
    ),
  );
  const versions = Array.from(
    new Set(
      rows
        .map((r) => {
          const error = record(r.error);
          return firstString(r.version, r.release, error.version, error.release);
        })
        .filter((v): v is string => !!v),
    ),
  );
  return {
    fingerprint: str(first.fingerprint) ?? params.fingerprint,
    message: str(first.message) ?? '',
    stack,
    recent_sessions: sessions,
    count: rows.length,
    users: users.size,
    first_seen_micros: timestamps[0] ?? 0,
    last_seen_micros: timestamps[timestamps.length - 1] ?? 0,
    pages,
    versions,
  };
}

/* ─────────── Performance ─────────── */

export async function webVitalsSeries(params: TimeRangeQueryParams): Promise<WebVitalsPoint[]> {
  const limit = params.limit ?? 200;
  const result = await runRumQuery({
    org_id: params.org_id,
    language: 'sql',
    statement: `SELECT * FROM rum_actions WHERE type = 'view' ORDER BY ts_micros ASC LIMIT ${limit}`,
    time_range: { start: params.from_micros, end: params.to_micros },
    stream: { name: 'rum_actions', stream_type: STREAM_TYPE },
    limit,
  });
  return toRecord(result).map((r) => {
    const payload = record(r.payload);
    const p: WebVitalsPoint = { ts_micros: num(r.ts_micros) ?? 0 };
    const stringFields: Array<[keyof WebVitalsPoint, string | undefined]> = [
      ['session_id', str(r.session_id)],
      ['page', actionPage(r, payload)],
      ['application', firstString(r.application, payload.application, payload.app)],
      ['environment', firstString(r.environment, payload.environment, payload.env)],
      ['version', firstString(r.version, r.release, payload.version, payload.release)],
      ['browser', firstString(r.browser, payload.browser)],
      ['country', firstString(r.country, payload.country)],
      ['device', firstString(r.device, payload.device)],
    ];
    for (const [key, value] of stringFields) {
      if (value !== undefined) {
        (p as unknown as Record<string, unknown>)[key] = value;
      }
    }
    const lcp = num(r.lcp_ms);
    const fid = num(r.fid_ms);
    const inp = num(r.inp_ms ?? payload.inp_ms);
    const cls = num(r.cls);
    const ttfb = num(r.ttfb_ms);
    if (lcp !== undefined) p.lcp_ms = lcp;
    if (fid !== undefined) p.fid_ms = fid;
    if (inp !== undefined) p.inp_ms = inp;
    if (cls !== undefined) p.cls = cls;
    if (ttfb !== undefined) p.ttfb_ms = ttfb;
    return p;
  });
}

export async function apiPerformance(params: TimeRangeQueryParams): Promise<ApiPerfRow[]> {
  const limit = params.limit ?? 50;
  const result = await runRumQuery({
    org_id: params.org_id,
    language: 'sql',
    statement: `SELECT url, COUNT(*) AS count, APPROX_PERCENTILE_CONT(duration_ms, 0.5) AS p50_ms, APPROX_PERCENTILE_CONT(duration_ms, 0.95) AS p95_ms, SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END)::DOUBLE / COUNT(*)::DOUBLE AS err_rate FROM rum_actions WHERE type = 'resource' GROUP BY url ORDER BY count DESC LIMIT ${limit}`,
    time_range: { start: params.from_micros, end: params.to_micros },
    stream: { name: 'rum_actions', stream_type: STREAM_TYPE },
    limit,
  });
  return toRecord(result)
    .map((r) => ({
      url: str(r.url) ?? '',
      count: num(r.count) ?? 0,
      p50_ms: num(r.p50_ms) ?? 0,
      p95_ms: num(r.p95_ms) ?? 0,
      err_rate: num(r.err_rate) ?? 0,
    }))
    .filter((row) => row.url.length > 0);
}

export async function errorRateSeries(params: TimeRangeQueryParams): Promise<Array<{ ts_micros: number; count: number }>> {
  const limit = params.limit ?? 200;
  const result = await runRumQuery({
    org_id: params.org_id,
    language: 'sql',
    statement: `SELECT timestamp AS ts_micros, COUNT(*) AS count FROM rum_errors GROUP BY timestamp ORDER BY timestamp ASC LIMIT ${limit}`,
    time_range: { start: params.from_micros, end: params.to_micros },
    stream: { name: 'rum_errors', stream_type: STREAM_TYPE },
    limit,
  });
  return toRecord(result).map((r) => ({
    ts_micros: num(r.ts_micros) ?? 0,
    count: num(r.count) ?? 0,
  }));
}
