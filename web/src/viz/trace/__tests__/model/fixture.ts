import type { Span, TraceResponse } from '@/api/web';

/**
 * The moment the fixture traces begin, in nanoseconds. Real epochs in
 * nanoseconds are past what a number holds exactly; this one is not, so
 * offsets come out exact.
 */
export const T0 = 1_786_704_600_000_000;
const MS = 1_000_000;

/** A span from `fromMs` to `toMs` after the trace began, with only what a test sets. */
export function span(
  id: string,
  parent: string | null,
  service: string,
  operation: string,
  fromMs: number,
  toMs: number,
  extra: Partial<Span> = {},
): Span {
  return {
    spanId: id,
    ...(parent ? { parentSpanId: parent } : {}),
    service,
    operation,
    startNs: T0 + fromMs * MS,
    endNs: T0 + toMs * MS,
    durationNs: (toMs - fromMs) * MS,
    kind: 1,
    status: 'OK',
    traceFlags: 1,
    traceState: '',
    resource: { attributes: {}, dropped_attributes_count: 0 },
    scope: { name: '', version: '', attributes: {}, dropped_attributes_count: 0 },
    attributes: {},
    events: [],
    links: [],
    droppedAttributesCount: 0,
    droppedEventsCount: 0,
    droppedLinksCount: 0,
    schemaVersion: 1,
    semanticConventionsVersion: '1.26.0',
    samplingReason: 'rule',
    partial: false,
    partialReasons: [],
    late: false,
    duplicate: false,
    conflict: false,
    ...extra,
  };
}

export function trace(spans: Span[], rootSpanId = spans[0]?.spanId ?? ''): TraceResponse {
  return {
    traceId: 'trace-1',
    rootSpanId: rootSpanId,
    spans,
    partial: false,
    partialReasons: [],
    samplingReasons: [],
    lateSpanCount: 0,
    duplicateSpanCount: 0,
    conflictSpanCount: 0,
  };
}

/**
 * A request to `checkout` that calls `inventory`, which runs two queries at
 * once, and then `payments`, which fails.
 */
export function checkoutTrace(): TraceResponse {
  return trace([
    span('root', null, 'checkout', 'POST /orders', 0, 100, {
      attributes: { 'http.response.status_code': 500, 'http.route': '/orders' },
    }),
    span('inv', 'root', 'inventory', 'GET /stock', 10, 50),
    span('q1', 'inv', 'postgres', 'SELECT stock', 12, 30),
    span('q2', 'inv', 'postgres', 'SELECT stock', 20, 40),
    span('pay', 'root', 'payments', 'POST /charge', 55, 95, { status: 'ERROR' }),
  ]);
}
