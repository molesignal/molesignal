/**
 * Answers for the RED view of the Traces explorer: `GET /api/v1/web/traces/red`
 * and `GET /api/v1/web/traces/red/breakdown`.
 *
 * The shared mock answers neither. `serveTraceRed` counts a day of Spans the
 * way the backend does: the requests of the scope asked for that pass the
 * query's filters and free text, laid out in the bars the page asked for, by
 * how long they took, and grouped by the values of a field. So a drill in a
 * spec changes what is counted, as it does against a real backend.
 *
 * Nothing is random: the Spans are worked out from the mock session's frozen
 * clock, so every run counts the same.
 */
import type { Page } from '@playwright/test';

import { FROZEN_NOW_ISO } from './mockBackend';

const MINUTE = 60_000_000;
const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const MS = 1_000_000;

type Value = string | number | null;

interface MockSpan {
  micros: number;
  trace: string;
  fields: Record<string, Value>;
}

/** What each kind of request is: the service that takes it in, how, and how long it usually takes. */
const REQUESTS = [
  { service: 'checkout-api', name: 'POST /api/orders', route: '/api/orders', method: 'POST', ms: 180 },
  { service: 'catalog', name: 'GET /api/products/:id', route: '/api/products/:id', method: 'GET', ms: 40 },
  { service: 'checkout-api', name: 'GET /api/cart', route: '/api/cart', method: 'GET', ms: 95 },
  { service: 'payment-gateway', name: 'POST /api/payments', route: '/api/payments', method: 'POST', ms: 2_400 },
  { service: 'checkout-api', name: 'POST /api/orders', route: '/api/orders', method: 'POST', ms: 320 },
  { service: 'web', name: 'GET /', route: '/', method: 'GET', ms: 12 },
];

/**
 * A trace every twelve minutes over the last day. Each is a request taken in
 * by its service (a server root), which calls inventory (a client Span and
 * inventory's own server Span) and reads the database (an internal Span).
 * One trace in six fails at the root, one in nine in inventory.
 */
const SPANS: MockSpan[] = Array.from({ length: 120 }, (_, nth) => {
  const micros = NOW - (nth + 0.5) * 12 * MINUTE;
  const trace = `trace-${String(nth).padStart(3, '0')}`;
  const request = REQUESTS[nth % REQUESTS.length]!;
  const failed = nth % 6 === 5;
  const inventoryFailed = nth % 9 === 4;
  const root = `${trace}-root`;
  const call = `${trace}-call`;
  const span = (id: string, fields: Record<string, Value>, offsetMicros: number): MockSpan => ({
    micros: micros + offsetMicros,
    trace,
    fields: { trace_id: trace, span_id: id, ...fields },
  });

  return [
    span(root, {
      parent_span_id: '',
      'service.name': request.service,
      name: request.name,
      kind: 2,
      status_code: failed ? 'ERROR' : 'OK',
      duration_ns: (request.ms + (nth % 7) * 10) * MS,
      'http.route': request.route,
      'http.request.method': request.method,
      'http.response.status_code': failed ? 500 : 200,
    }, 0),
    span(call, {
      parent_span_id: root,
      'service.name': request.service,
      name: 'GET inventory',
      kind: 3,
      status_code: inventoryFailed ? 'ERROR' : 'UNSET',
      duration_ns: (18 + (nth % 5)) * MS,
      'http.request.method': 'GET',
    }, 1_000),
    span(`${trace}-stock`, {
      parent_span_id: call,
      'service.name': 'inventory',
      name: 'GET /stock',
      kind: 2,
      status_code: inventoryFailed ? 'ERROR' : 'OK',
      duration_ns: (15 + (nth % 5)) * MS,
      'http.route': '/stock',
      'http.request.method': 'GET',
      'http.response.status_code': inventoryFailed ? 503 : 200,
    }, 2_000),
    span(`${trace}-db`, {
      parent_span_id: root,
      'service.name': request.service,
      name: 'db.query',
      kind: 1,
      status_code: 'UNSET',
      duration_ns: (2 + (nth % 3)) * MS,
    }, 30_000),
  ];
}).flat();

/** The edges of the duration bars, as the backend counts them: the R10 series from 100 ns to 1,000 s. */
const PREFERRED = [100, 125, 160, 200, 250, 315, 400, 500, 630, 800];
const EDGES = [
  ...Array.from({ length: 10 }, (_, decade) => PREFERRED.map((edge) => edge * 10 ** decade)).flat(),
  100 * 10 ** 10,
];

interface Filter {
  field: string;
  op: string;
  value: string;
}

/** The condition tree beside the filters: a filter, text to find, or a group that all or one of holds. */
type Condition = Filter | { text: string } | { and: Condition[] } | { or: Condition[] };

const failed = (span: MockSpan) => String(span.fields.status_code ?? '').toUpperCase() === 'ERROR';

function perTrace(count: (spans: MockSpan[]) => number): Map<string, number> {
  const byTrace = new Map<string, MockSpan[]>();
  for (const span of SPANS) byTrace.set(span.trace, [...(byTrace.get(span.trace) ?? []), span]);

  return new Map([...byTrace].map(([trace, spans]) => [trace, count(spans)]));
}

const SPAN_COUNTS = perTrace((spans) => spans.length);
const ERROR_COUNTS = perTrace((spans) => spans.filter(failed).length);

function compare(actual: Value, op: string, expected: string): boolean {
  if (op === 'contains') return actual !== null && String(actual).includes(expected);
  if (typeof actual === 'number' || (op !== '=' && op !== '!=')) {
    const left = Number(actual);
    const right = Number(expected);
    if (actual === null || Number.isNaN(left)) return op === '!=';
    if (op === '=') return left === right;
    if (op === '!=') return left !== right;
    if (op === '>') return left > right;
    if (op === '>=') return left >= right;
    if (op === '<') return left < right;
    return left <= right;
  }
  const text = actual === null ? null : String(actual);

  // A Span without the field does not have the value left out, as with a number above.
  return op === '=' ? text === expected : text !== expected;
}

/** Whether a Span is a request the query asked for: of the scope, passing every filter and the free text. */
function counted(span: MockSpan, params: URLSearchParams): boolean {
  if (params.get('scope') !== 'all') {
    const { kind, parent_span_id: parent } = span.fields;
    // A client or producer Span without a parent is a call made outside any request.
    const root = (parent === '' || parent === null) && kind !== 3 && kind !== 4;
    if (kind !== 2 && kind !== 5 && !root) return false;
  }

  const filters = JSON.parse(params.get('filters') ?? '[]') as Filter[];
  if (!filters.every((filter) => holds(span, filter))) return false;

  const text = params.get('q')?.trim();
  if (text && !found(span, text)) return false;

  // The tree is asked of the Span itself, as each filter is.
  const where = params.get('where');
  return where === null || holds(span, JSON.parse(where) as Condition);
}

/** Whether a Span has text where the trace list looks for it, whatever the case. */
function found(span: MockSpan, text: string): boolean {
  return ['trace_id', 'span_id', 'service.name', 'name', 'status_code'].some((field) =>
    String(span.fields[field] ?? '').toLowerCase().includes(text.toLowerCase()),
  );
}

function holds(span: MockSpan, condition: Condition): boolean {
  if ('and' in condition) return condition.and.every((item) => holds(span, item));
  if ('or' in condition) return condition.or.some((item) => holds(span, item));
  if ('text' in condition) return found(span, condition.text);

  // A filter names its field as the stream does, as the backend reads it.
  const field = condition.field;
  const actual =
    field === 'span_count'
      ? (SPAN_COUNTS.get(span.trace) ?? 0)
      : field === 'error_count'
        ? (ERROR_COUNTS.get(span.trace) ?? 0)
        : (span.fields[field] ?? null);

  return compare(actual, condition.op, condition.value);
}

/** The nearest-rank percentile of durations already sorted. */
function percentile(sorted: number[], share: number): number | null {
  if (sorted.length === 0) return null;

  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(share * sorted.length) - 1))]!;
}

function figures(spans: MockSpan[]) {
  const durations = spans.map((span) => Number(span.fields.duration_ns)).sort((a, b) => a - b);

  return {
    requests: spans.length,
    errors: spans.filter(failed).length,
    p50Ns: percentile(durations, 0.5),
    p95Ns: percentile(durations, 0.95),
    p99Ns: percentile(durations, 0.99),
  };
}

interface Bars {
  from: number;
  to: number;
  bucket: number;
  origin: number;
}

function barsOf(params: URLSearchParams): Bars {
  return {
    from: Number(params.get('from')),
    to: Number(params.get('to')),
    bucket: Number(params.get('bucketTime')),
    origin: Number(params.get('originTime')),
  };
}

/** The Spans each bar holds, by where the bar begins, oldest first. */
function byBar(spans: MockSpan[], bars: Bars): Array<[number, MockSpan[]]> {
  const held = new Map<number, MockSpan[]>();
  for (const span of spans) {
    const start = bars.origin + Math.floor((span.micros - bars.origin) / bars.bucket) * bars.bucket;
    held.set(start, [...(held.get(start) ?? []), span]);
  }

  return [...held].sort(([a], [b]) => a - b);
}

function requestsOf(params: URLSearchParams): MockSpan[] {
  const bars = barsOf(params);

  return SPANS.filter((span) => span.micros >= bars.from && span.micros < bars.to && counted(span, params));
}

function overview(params: URLSearchParams) {
  const bars = barsOf(params);
  const spans = requestsOf(params);
  const latency = new Map<number, MockSpan[]>();
  for (const span of spans) {
    const duration = Number(span.fields.duration_ns);
    const index = Math.max(0, EDGES.filter((edge) => edge <= duration).length - 1);
    latency.set(index, [...(latency.get(index) ?? []), span]);
  }

  return {
    bucketTime: bars.bucket,
    originTime: bars.origin,
    scope: params.get('scope') === 'all' ? 'all' : 'entry',
    total: figures(spans),
    buckets: byBar(spans, bars).map(([start, held]) => ({ startTime: start, ...figures(held) })),
    latency: [...latency]
      .sort(([a], [b]) => a - b)
      .map(([index, held]) => ({
        fromNs: index === 0 ? 0 : EDGES[index]!,
        toNs: EDGES[index + 1] ?? null,
        requests: held.length,
        errors: held.filter(failed).length,
      })),
  };
}

function breakdown(params: URLSearchParams) {
  const bars = barsOf(params);
  const field = params.get('by') ?? '';
  const groups = new Map<string | null, MockSpan[]>();
  for (const span of requestsOf(params)) {
    const raw = span.fields[field];
    const value = raw === null || raw === undefined || raw === '' ? null : String(raw);
    groups.set(value, [...(groups.get(value) ?? []), span]);
  }
  const ranked = [...groups].sort(
    ([a, left], [b, right]) => right.length - left.length || String(a ?? '￿').localeCompare(String(b ?? '￿')),
  );

  return {
    field,
    bucketTime: bars.bucket,
    originTime: bars.origin,
    scope: params.get('scope') === 'all' ? 'all' : 'entry',
    groups: ranked.slice(0, 50).map(([value, spans]) => ({
      value,
      ...figures(spans),
      series: byBar(spans, bars).map(([start, held]) => {
        const { requests, errors, p95Ns } = figures(held);

        return { startTime: start, requests, errors, p95Ns: p95Ns };
      }),
    })),
    more: ranked.length > 50,
  };
}

/** What the RED view asked, each endpoint in the order it was asked. */
export interface AskedRed {
  overview: URLSearchParams[];
  breakdown: URLSearchParams[];
}

/** Serves the RED view from the day of Spans above. Register it after `mountMockRoutes`. */
export async function serveTraceRed(page: Page): Promise<AskedRed> {
  const asked: AskedRed = { overview: [], breakdown: [] };

  await page.route(/\/api\/v1\/web\/traces\/red(\?.*)?$/, async (route) => {
    const params = new URL(route.request().url()).searchParams;
    asked.overview.push(params);
    await route.fulfill({ json: overview(params) });
  });
  await page.route(/\/api\/v1\/web\/traces\/red\/breakdown(\?.*)?$/, async (route) => {
    const params = new URL(route.request().url()).searchParams;
    asked.breakdown.push(params);
    await route.fulfill({ json: breakdown(params) });
  });

  return asked;
}
